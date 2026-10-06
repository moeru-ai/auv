import type { Range } from '@codemirror/state'
import type { DecorationSet, ViewUpdate } from '@codemirror/view'

import type { HoverInfo } from '../runtime/protocol'
import type { PlaygroundState } from '../store'

import { autocompletion, closeBrackets, closeBracketsKeymap, completionKeymap, startCompletion } from '@codemirror/autocomplete'
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands'
import { javascript } from '@codemirror/lang-javascript'
import { bracketMatching, HighlightStyle, indentOnInput, syntaxHighlighting } from '@codemirror/language'
import { linter, lintKeymap } from '@codemirror/lint'
import { highlightSelectionMatches, searchKeymap } from '@codemirror/search'
import { EditorState, Prec, RangeSet, StateEffect, StateField } from '@codemirror/state'
import { Decoration, EditorView, gutter, GutterMarker, highlightActiveLine, highlightActiveLineGutter, hoverTooltip, keymap, lineNumbers, tooltips, WidgetType } from '@codemirror/view'
import { tags } from '@lezer/highlight'
import { useEffect, useRef } from 'react'
import { createRoot } from 'react-dom/client'

import { language, session } from '../runtime/session'
import { actions, usePlayground } from '../store'
import { bindHistory, lineAt as timelineLineAt } from '../timeline'
import { HoverCard } from './HoverCard'

const STORAGE_KEY = 'auv-playground:source'

export const DEFAULT_SOURCE = `// ⌘↩ run all · ⇧↩ run line · ⌘⇧↩ step from the top
// Click the gutter left of a line number to toggle a breakpoint.

const todos = await auv.windows.resolve({ appName: 'Todos' })
const counter = await auv.windows.resolve({ appName: 'Counter' })

// OCR the todo window and mark every open item as done.
const before = await todos.findText('Done')
for (const match of before.matches) {
  const point = centerOf(match.bounds)
  await auv.input.click(point)
}

// Click a button a few times and read the result back with OCR.
for (let i = 0; i < 3; i++) {
  const button = await counter.findText('Increment')
  await auv.input.click(centerOf(button.matches[0]!.bounds))
}
const shot = await counter.capture()
const count = await auv.text.recognize(shot)
show(count.text, 'counter text')
`

interface RunView {
  breakpoints: number[]
  currentLine: null | number
  /** Line executing at the time cursor, when the cursor is not following the latest event. */
  cursorLine: null | number
  errorLine?: number
  executedLines: number[]
  status: PlaygroundState['run']['status']
  timings: PlaygroundState['timings']
}

const setRunView = StateEffect.define<RunView>()

class BreakpointMarker extends GutterMarker {
  constructor(readonly active: boolean, readonly current: boolean) {
    super()
  }

  eq(other: BreakpointMarker): boolean {
    return other.active === this.active && other.current === this.current
  }

  toDOM(): Node {
    if (this.current) {
      const arrow = document.createElement('span')
      arrow.className = 'cm-arrow'
      arrow.textContent = '▶'
      return arrow
    }
    const dot = document.createElement('span')
    dot.className = this.active ? 'cm-bp' : 'cm-bp-ghost'
    return dot
  }
}

class TimingWidget extends WidgetType {
  constructor(readonly text: string, readonly hot: boolean, readonly top: boolean) {
    super()
  }

  eq(other: TimingWidget): boolean {
    return other.text === this.text && other.hot === this.hot && other.top === this.top
  }

  toDOM(): HTMLElement {
    const span = document.createElement('span')
    span.className = 'cm-run-timing'
    span.dataset.hot = String(this.hot)
    span.dataset.top = String(this.top)
    // Rendered by `::before` so selecting or copying code never includes the timing.
    span.dataset.text = this.text
    span.setAttribute('aria-label', this.text)
    return span
  }
}

const ghostMarker = new BreakpointMarker(false, false)

const runField = StateField.define<{ decorations: DecorationSet, markers: RangeSet<GutterMarker>, view?: RunView }>({
  create: () => ({ decorations: Decoration.none, markers: RangeSet.empty }),
  provide: field => [
    EditorView.decorations.from(field, value => value.decorations),
  ],
  update(value, transaction) {
    for (const effect of transaction.effects) {
      if (effect.is(setRunView))
        return { ...buildDecorations(transaction.state, effect.value), view: effect.value }
    }
    if (!transaction.docChanged)
      return value
    return { ...value, decorations: value.decorations.map(transaction.changes), markers: value.markers.map(transaction.changes) }
  },
})

function buildDecorations(state: EditorState, run: RunView): { decorations: DecorationSet, markers: RangeSet<GutterMarker> } {
  const decorations: Range<Decoration>[] = []
  const markers: Range<GutterMarker>[] = []
  const lines = state.doc.lines
  const lineAt = (line: number) => state.doc.line(Math.min(Math.max(line, 1), lines))

  const executed = new Set(run.executedLines)
  for (const line of executed) {
    if (line <= lines && line !== run.currentLine)
      decorations.push(Decoration.line({ class: 'cm-run-executed' }).range(lineAt(line).from))
  }
  if (run.currentLine !== null && run.currentLine <= lines && (run.status === 'paused' || run.status === 'running')) {
    decorations.push(Decoration.line({ class: run.status === 'paused' ? 'cm-run-paused' : 'cm-run-current' }).range(lineAt(run.currentLine).from))
  }
  if (run.status === 'error' && run.errorLine && run.errorLine <= lines)
    decorations.push(Decoration.line({ class: 'cm-run-error' }).range(lineAt(run.errorLine).from))
  if (run.cursorLine !== null && run.cursorLine <= lines)
    decorations.push(Decoration.line({ class: 'cm-time-cursor' }).range(lineAt(run.cursorLine).from))

  // Inline timings: inclusive time on top-level statements, self time × hits elsewhere.
  const widgets = new Map<number, { hot: boolean, text: string, top: boolean }>()
  for (const line of run.timings.lines) {
    if (line.selfMs < 0.5 && line.hits <= 1)
      continue
    widgets.set(line.line, { hot: line.selfMs > 500, text: `${formatMs(line.selfMs)}${line.hits > 1 ? ` ×${line.hits}` : ''}`, top: false })
  }
  const statements = new Map<number, number>()
  for (const statement of run.timings.statements)
    statements.set(statement.line, (statements.get(statement.line) ?? 0) + statement.ms)
  for (const statement of run.timings.statements) {
    if (statement.endLine === statement.line)
      continue
    const total = statements.get(statement.line) ?? statement.ms
    widgets.set(statement.line, { hot: total > 1000, text: `Σ ${formatMs(total)}`, top: true })
  }
  for (const [line, widget] of widgets) {
    if (line <= lines)
      decorations.push(Decoration.widget({ side: 1, widget: new TimingWidget(widget.text, widget.hot, widget.top) }).range(lineAt(line).to))
  }

  const breakpointSet = new Set(run.breakpoints)
  const markerLines = new Set([...run.breakpoints, ...(run.status === 'paused' && run.currentLine ? [run.currentLine] : [])])
  for (const line of [...markerLines].sort((a, b) => a - b)) {
    if (line <= lines)
      markers.push(new BreakpointMarker(breakpointSet.has(line), run.status === 'paused' && line === run.currentLine).range(lineAt(line).from))
  }

  return { decorations: Decoration.set(decorations, true), markers: RangeSet.of(markers, true) }
}

function formatMs(ms: number): string {
  if (ms < 1)
    return '<1ms'
  if (ms < 1000)
    return `${Math.round(ms)}ms`
  return `${(ms / 1000).toFixed(ms < 10_000 ? 2 : 1)}s`
}

const stepGutter = gutter({
  class: 'cm-step-gutter',
  domEventHandlers: {
    mousedown(view, block) {
      const line = view.state.doc.lineAt(block.from).number
      session.setBreakpoints(actions.toggleBreakpoint(line))
      return true
    },
  },
  initialSpacer: () => ghostMarker,
  lineMarker: (view, block) => {
    const markers = view.state.field(runField).markers
    let found: GutterMarker | null = null
    markers.between(block.from, block.from, (_from, _to, marker) => {
      found = marker
    })
    return found ?? ghostMarker
  },
  lineMarkerChange: update => update.transactions.some(transaction => transaction.effects.some(effect => effect.is(setRunView))),
})

const highlight = HighlightStyle.define([
  // Colors are CSS variables so the same style follows the light/dark theme.
  { color: 'var(--syn-keyword)', tag: [tags.keyword, tags.modifier, tags.controlKeyword, tags.operatorKeyword] },
  { color: 'var(--syn-function)', tag: [tags.function(tags.variableName), tags.function(tags.propertyName)] },
  { color: 'var(--syn-string)', tag: [tags.string, tags.special(tags.string)] },
  { color: 'var(--syn-number)', tag: [tags.number, tags.bool, tags.null] },
  { color: 'var(--syn-type)', tag: [tags.typeName, tags.className, tags.namespace] },
  { color: 'var(--syn-variable)', tag: [tags.variableName, tags.propertyName] },
  { color: 'var(--syn-comment)', fontStyle: 'italic', tag: tags.comment },
  { color: 'var(--syn-punctuation)', tag: [tags.operator, tags.punctuation, tags.bracket] },
])

/** Runs the selection, or the whole document when nothing is selected. */
function runAll(view: EditorView, request: 'replay' | 'run' | 'step'): boolean {
  const selection = view.state.selection.main
  const mode = request === 'step' ? 'step' : 'run'
  const replay = request === 'replay'
  if (selection.empty) {
    void session.run({ mode, replay, source: view.state.doc.toString(), startLine: 1 })
    return true
  }
  const from = view.state.doc.lineAt(selection.from)
  const to = view.state.doc.lineAt(selection.to)
  void session.run({ mode, replay, source: view.state.sliceDoc(from.from, to.to), startLine: from.number })
  return true
}

/** Runs the current line (REPL style) and moves the cursor to the next line. */
function runLine(view: EditorView): boolean {
  const selection = view.state.selection.main
  if (!selection.empty)
    return runAll(view, 'run')
  const line = view.state.doc.lineAt(selection.head)
  void session.run({ mode: 'run', source: line.text, startLine: line.number })
  const next = Math.min(line.number + 1, view.state.doc.lines)
  view.dispatch({ scrollIntoView: true, selection: { anchor: view.state.doc.line(next).from } })
  return true
}

const runKeymap = Prec.highest(keymap.of([
  { key: 'Mod-Enter', run: view => runAll(view, 'run') },
  { key: 'Mod-Shift-Enter', run: view => runAll(view, 'step') },
  { key: 'Shift-Enter', run: runLine },
  // VS Code's macOS Trigger Suggest alternative; `Ctrl-Space` comes from `completionKeymap`.
  { key: 'Alt-Escape', run: startCompletion },
]))

/** Maps TypeScript `ScriptElementKind` to CodeMirror completion icons. */
const COMPLETION_TYPES: Record<string, string> = {
  'alias': 'namespace',
  'class': 'class',
  'const': 'constant',
  'enum': 'enum',
  'function': 'function',
  'getter': 'property',
  'interface': 'interface',
  'keyword': 'keyword',
  'let': 'variable',
  'local function': 'function',
  'local var': 'variable',
  'method': 'method',
  'module': 'namespace',
  'parameter': 'variable',
  'property': 'property',
  'setter': 'property',
  'type': 'type',
  'var': 'variable',
}

/** Side panel for the selected completion: its signature and JSDoc. */
async function completionInfo(pos: number, name: string, source: string): Promise<HTMLElement | null> {
  const detail = await language.completionDetail(pos, name, source)
  if (!detail)
    return null
  const dom = document.createElement('div')
  dom.className = 'cm-completion-detail'
  const type = document.createElement('code')
  type.textContent = detail.type
  dom.append(type)
  if (detail.doc) {
    const doc = document.createElement('p')
    doc.textContent = detail.doc
    dom.append(doc)
  }
  return dom
}

const hover = hoverTooltip(async (view, pos) => {
  const line = view.state.doc.lineAt(pos)
  const word = view.state.wordAt(pos)
  const name = word ? view.state.sliceDoc(word.from, word.to) : undefined
  const state = usePlayground.getState()
  const hasValue = name !== undefined && (name in state.vars || bindHistory(state, name).length > 0)
  const hasCalls = state.calls.some(call => call.line === line.number)
  let info: HoverInfo | null = null
  try {
    info = await language.hover(pos, view.state.doc.toString())
  }
  catch {}
  if (!info && !hasValue && !hasCalls)
    return null
  return {
    // Prefer below the hovered line (CodeMirror flips only when it does not fit),
    // so sweeping across tokens does not alternate between above and below.
    above: false,
    create() {
      const dom = document.createElement('div')
      const root = createRoot(dom)
      root.render(<HoverCard info={info} line={line.number} name={hasValue ? name : undefined} />)
      return { destroy: () => root.unmount(), dom }
    },
    end: info?.to ?? word?.to ?? pos,
    pos: info?.from ?? word?.from ?? pos,
  }
}, { hideOnChange: true, hoverTime: 280 })

export function CodeEditor() {
  const hostRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    let saved: null | string = null
    try {
      saved = localStorage.getItem(STORAGE_KEY)
    }
    catch {}
    const initial = saved ?? DEFAULT_SOURCE
    let timer: number | undefined
    let ready = false

    const view = new EditorView({
      parent: hostRef.current!,
      state: EditorState.create({
        doc: initial,
        extensions: [
          runKeymap,
          // Render tooltips (hover, completion, completion info) in <body> so the
          // editor panel's overflow clipping and neighbouring panels never hide them.
          tooltips({ parent: document.body }),
          stepGutter,
          lineNumbers(),
          highlightActiveLineGutter(),
          highlightActiveLine(),
          history(),
          indentOnInput(),
          bracketMatching(),
          closeBrackets(),
          highlightSelectionMatches(),
          javascript({ typescript: true }),
          syntaxHighlighting(highlight),
          runField,
          hover,
          linter(async () => (ready ? await language.diagnostics() : []), { delay: 350 }),
          autocompletion({
            override: [async (context) => {
              const word = context.matchBefore(/[\w$]*/)
              if (!word || !ready)
                return null
              const afterDot = context.state.sliceDoc(word.from - 1, word.from) === '.'
              if (word.from === word.to && !context.explicit && !afterDot)
                return null
              // Send the live document: the debounced worker copy may not have the `.` yet.
              const source = context.state.doc.toString()
              const result = await language.completions(context.pos, source)
              if (context.aborted)
                return null
              return {
                from: word.from,
                options: result.items.map(item => ({
                  boost: Math.max(-99, 30 - Number.parseInt(item.sortText, 10) * 5),
                  info: () => completionInfo(context.pos, item.label, source),
                  label: item.label,
                  type: COMPLETION_TYPES[item.kind] ?? 'variable',
                })),
                validFor: /^[\w$]*$/,
              }
            }],
          }),
          keymap.of([...closeBracketsKeymap, ...defaultKeymap, ...searchKeymap, ...historyKeymap, ...completionKeymap, ...lintKeymap, indentWithTab]),
          EditorView.updateListener.of((update: ViewUpdate) => {
            if (!update.docChanged)
              return
            window.clearTimeout(timer)
            timer = window.setTimeout(() => {
              const source = update.state.doc.toString()
              void language.update(source)
              try {
                localStorage.setItem(STORAGE_KEY, source)
              }
              catch {}
            }, 150)
          }),
          EditorView.theme({
            '&': { background: 'var(--c-surface-1)', color: 'var(--c-fg)' },
            '.cm-content': { caretColor: 'var(--c-accent)', paddingBottom: '40vh' },
            '.cm-cursor': { borderLeftColor: 'var(--c-accent)' },
          }),
        ],
      }),
    })

    // NOTICE(dev-test-hook): exposes the editor to browser automation in dev
    // builds only; stripped from production bundles by Vite.
    if (import.meta.env.DEV)
      (window as unknown as { __auvEditor?: EditorView }).__auvEditor = view

    void language.init(initial).then(() => {
      ready = true
    })

    const sync = (state: PlaygroundState) => {
      view.dispatch({
        effects: setRunView.of({
          breakpoints: state.breakpoints,
          currentLine: state.run.currentLine,
          cursorLine: state.cursor === null ? null : timelineLineAt(state, state.cursor),
          errorLine: state.run.errorLine,
          executedLines: state.executedLines,
          status: state.run.status,
          timings: state.timings,
        }),
      })
    }
    sync(usePlayground.getState())
    const unsubscribe = usePlayground.subscribe((state, previous) => {
      if (state.run !== previous.run || state.timings !== previous.timings || state.executedLines !== previous.executedLines || state.breakpoints !== previous.breakpoints || state.cursor !== previous.cursor)
        sync(state)
    })

    // Header buttons run the editor's current selection or document.
    const onRunRequest = (event: Event) => {
      runAll(view, (event as CustomEvent<'replay' | 'run' | 'step'>).detail)
    }
    document.addEventListener('playground:run', onRunRequest)

    return () => {
      document.removeEventListener('playground:run', onRunRequest)
      unsubscribe()
      window.clearTimeout(timer)
      view.destroy()
    }
  }, [])

  return <div className="h-full overflow-hidden" ref={hostRef} />
}
