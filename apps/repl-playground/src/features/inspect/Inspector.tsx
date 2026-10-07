import type { AxNode } from '../../backend/types'
import type { InspectorTab, Resource } from '../../store'

import { Tabs } from '@base-ui/react/tabs'
import { useEffect, useMemo, useRef, useState } from 'react'

import { Empty } from '../../components/Empty'
import { IconButton } from '../../components/IconButton'
import { TabBar } from '../../components/TabBar'
import { refHint } from '../../handles'
import { session } from '../../runtime/session'
import { actions, usePlayground } from '../../store'
import { bindingsOf, consumersOf, cursorSeq, varsAt } from '../../timeline'
import { ResourcePreview } from './ResourcePreview'
import { RefChip, ValueView } from './ValueView'

interface RefGroup {
  /** App name for a window group; `undefined` for handles that are not windows. */
  app?: string
  bundleId?: string
  key: string
  refs: string[]
}

/** Right-hand inspector: the selected handle with its lineage, and the AX tree. */
export function Inspector() {
  const tab = usePlayground(state => state.inspectorTab)
  return (
    <Tabs.Root className="bg-surface-1 flex flex-col h-full" onValueChange={value => usePlayground.setState({ inspectorTab: value as InspectorTab })} value={tab}>
      <TabBar
        tabs={[
          { label: 'Handle', value: 'handle' },
          { label: 'Call', value: 'call' },
          { label: 'AX tree', value: 'ax' },
        ]}
      />
      <Tabs.Panel className="flex-1 min-h-0" value="handle"><HandlePanel /></Tabs.Panel>
      <Tabs.Panel className="flex-1 min-h-0" value="call"><CallPanel /></Tabs.Panel>
      <Tabs.Panel className="flex-1 min-h-0" value="ax"><AxTreePanel /></Tabs.Panel>
    </Tabs.Root>
  )
}

/** Bound values as of the time cursor (latest when following), plus persisted top-level names. */
export function VariablesPanel() {
  const vars = usePlayground(state => state.vars)
  const binds = usePlayground(state => state.binds)
  const cursor = usePlayground(cursorSeq)
  const following = usePlayground(state => state.cursor === null)
  const entries = useMemo(() => {
    const asOf = binds.length > 0 ? varsAt(usePlayground.getState(), cursor) : {}
    return Object.entries(following ? { ...asOf, ...vars } : asOf)
  }, [binds, cursor, following, vars])

  if (entries.length === 0)
    return <Empty>Declared names show up here; scrub the timeline to see them at any moment.</Empty>
  return (
    <div className="text-[12.5px] font-mono px-3 py-2 h-full overflow-auto">
      {!following && <div className="text-[11px] text-[var(--o-hover)] mb-1">{`as of seq ${cursor}`}</div>}
      {entries.map(([name, value]) => (
        <div className="leading-6 py-1.5 border-b border-line/50 flex gap-2" key={name}>
          <span className="text-fg-muted shrink-0">{name}</span>
          <span className="text-fg-subtle">=</span>
          <div className="min-w-0"><ValueView value={value} /></div>
        </div>
      ))}
    </div>
  )
}

function AxRow({ collapsed, depth, node, onToggle, selectedPath }: {
  collapsed: Set<string>
  depth: number
  node: AxNode
  onToggle: (path: string) => void
  selectedPath: null | string
}) {
  const ref = useRef<HTMLDivElement>(null)
  const isSelected = node.path === selectedPath
  // Ancestors of the selected node stay open so a canvas pick is always visible.
  const isCollapsed = collapsed.has(node.path) && !selectedPath?.startsWith(`${node.path}/`)

  useEffect(() => {
    if (isSelected)
      ref.current?.scrollIntoView({ block: 'nearest' })
  }, [isSelected])

  return (
    <>
      <div
        className={`pr-2 flex gap-1 h-6 cursor-default whitespace-nowrap items-center ${isSelected ? 'bg-accent/18' : 'hover:bg-surface-2'}`}
        onClick={() => usePlayground.setState({ selectedAxPath: node.path })}
        onMouseEnter={() => usePlayground.setState({ hoveredRect: node.frame ?? null })}
        ref={ref}
        style={{ paddingLeft: 8 + depth * 14 }}
      >
        <button
          className={`text-fg-subtle size-4 cursor-pointer ${node.children.length === 0 ? 'invisible' : ''}`}
          onClick={(event) => {
            event.stopPropagation()
            onToggle(node.path)
          }}
          type="button"
        >
          {isCollapsed ? '▸' : '▾'}
        </button>
        <span className="text-syn-type">{node.role}</span>
        {node.label && <span className="text-syn-string truncate">{JSON.stringify(node.label)}</span>}
        {node.value && <span className="text-fg-subtle truncate">{`= ${JSON.stringify(node.value)}`}</span>}
        {node.actions && <span className="text-[10.5px] text-syn-keyword">{node.actions.join(' ')}</span>}
      </div>
      {!isCollapsed && node.children.map(child => (
        <AxRow collapsed={collapsed} depth={depth + 1} key={child.path} node={child} onToggle={onToggle} selectedPath={selectedPath} />
      ))}
    </>
  )
}

function AxTreePanel() {
  const tree = usePlayground(state => state.axTree)
  const backend = usePlayground(state => state.backend)
  const selectedPath = usePlayground(state => state.selectedAxPath)
  const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set())
  const selected = tree && selectedPath ? findNode(tree, selectedPath) : undefined

  if (!backend?.accessibilityTree) {
    return (
      <Empty>
        The current device does not provide an accessibility tree yet. AUV has no AX snapshot capability exposed to clients; the mock desktop shows what it will look like.
      </Empty>
    )
  }

  return (
    <div className="flex flex-col h-full">
      <div className="text-[11.5px] text-fg-subtle px-2 border-b border-line flex shrink-0 gap-1 h-8 items-center">
        <span className="flex-1">Hover to highlight · click the canvas to pick</span>
        <IconButton hint="Refresh the accessibility tree" icon="i-ph-arrows-clockwise" onClick={() => void session.refreshAxTree()} />
      </div>
      <div className="text-[12.5px] font-mono py-1 flex-1 min-h-0 overflow-auto" onMouseLeave={() => usePlayground.setState({ hoveredRect: null })}>
        {tree
          ? (
              <AxRow
                collapsed={collapsed}
                depth={0}
                node={tree}
                onToggle={path => setCollapsed((current) => {
                  const next = new Set(current)
                  if (next.has(path))
                    next.delete(path)
                  else
                    next.add(path)
                  return next
                })}
                selectedPath={selectedPath}
              />
            )
          : <Empty>Loading…</Empty>}
      </div>
      {selected && (
        <div className="text-[12px] p-3 border-t border-line shrink-0 max-h-40 overflow-auto">
          <ValueView value={{ actions: selected.actions, frame: selected.frame, label: selected.label, path: selected.path, role: selected.role, value: selected.value }} />
        </div>
      )}
    </div>
  )
}

/** The selected call: arguments, outcome, and every handle it produced (filterable). */
function CallPanel() {
  const call = usePlayground(state => state.calls.find(candidate => candidate.id === state.selectedCallId))
  const resources = usePlayground(state => state.resources)
  const [query, setQuery] = useState('')
  const refs = useMemo(() => {
    const needle = query.trim().toLowerCase()
    if (!call || !needle)
      return call?.refs ?? []
    return call.refs.filter((ref) => {
      const resource = resources[ref]
      const window = resource?.kind === 'window' ? `${resource.handle.title ?? ''} ${resource.handle.bundleId ?? ''}` : ''
      return `${ref} ${refHint(resource) ?? ''} ${window}`.toLowerCase().includes(needle)
    })
  }, [call, query, resources])

  if (!call)
    return <Empty>Click a row in Calls to inspect it here.</Empty>
  const duration = call.endedAt === undefined ? undefined : Math.round(call.endedAt - call.startedAt)

  return (
    <div className="text-[12.5px] flex flex-col h-full">
      <div className="p-3 border-b border-line flex shrink-0 flex-col gap-2">
        <div className="font-mono flex gap-2 items-center">
          <span className={`rounded-full size-1.5 ${call.effect === 'input' ? 'bg-kind-input' : 'bg-accent'}`} title={call.effect} />
          <span className={call.status === 'error' ? 'text-bad' : 'text-fg'}>{call.method}</span>
          <button
            className="text-[11px] text-fg-muted ml-auto px-1.5 rounded bg-surface-2 cursor-pointer hover:text-fg"
            onClick={() => actions.setCursor(call.endSeq ?? call.seq)}
            title="Move the time cursor to this call"
            type="button"
          >
            {`${call.line === null ? '' : `L${call.line} `}#${call.hit} · seq ${call.seq}`}
          </button>
        </div>
        <div className="text-[11.5px] text-fg-subtle font-mono">
          {`${call.status}${duration === undefined ? '' : ` · ${duration}ms`} · ${call.effect}`}
        </div>
        {call.args.length > 0 && (
          <div className="flex flex-col gap-1">
            <div className="panel-title">Arguments</div>
            {call.args.map((arg, index) => (
              // eslint-disable-next-line react/no-array-index-key -- arguments are positional
              <div className="font-mono" key={index}><ValueView value={arg} /></div>
            ))}
          </div>
        )}
        {call.error && <div className="text-bad font-mono px-2 py-1.5 rounded-md bg-bad/10 [overflow-wrap:anywhere]">{call.error}</div>}
      </div>

      {call.refs.length > 0
        ? (
            <>
              <div className="px-3 py-2 border-b border-line flex shrink-0 flex-col gap-2">
                <span className="panel-title">{`Handles ${refs.length === call.refs.length ? call.refs.length : `${refs.length}/${call.refs.length}`}`}</span>
                {call.refs.length > 8 && (
                  <input
                    aria-label="Filter handles"
                    className="input text-[12px] h-7"
                    onChange={event => setQuery(event.target.value)}
                    placeholder="Filter by title, app, bundle or id"
                    value={query}
                  />
                )}
              </div>
              <div className="pb-2 flex-1 min-h-0 overflow-auto">
                {groupByApp(refs, resources).map(group => (
                  <section key={group.key}>
                    {group.app !== undefined && (
                      <div className="text-[11.5px] px-3 pb-1 pt-2.5 bg-surface-1 flex gap-1.5 items-baseline top-0 sticky z-1">
                        <span className="text-fg font-medium truncate">{group.app || '(unknown app)'}</span>
                        {group.bundleId && <span className="text-[10.5px] text-fg-subtle font-mono truncate">{group.bundleId}</span>}
                        <span className="text-[10.5px] text-fg-subtle font-mono ml-auto shrink-0">{group.refs.length}</span>
                      </div>
                    )}
                    {group.refs.map((ref) => {
                      const resource = resources[ref]
                      if (resource?.kind !== 'window')
                        return <div className="px-3 py-0.5" key={ref}><RefChip refId={ref} /></div>
                      return (
                        <RefChip
                          className="text-[12px] pl-5 pr-3 text-left flex gap-2 h-6 w-full cursor-pointer items-center hover:bg-surface-2"
                          key={ref}
                          refId={ref}
                        >
                          <span className={`flex-1 truncate ${resource.handle.title ? 'text-fg' : 'text-fg-subtle italic'}`}>{resource.handle.title || '(untitled)'}</span>
                          <span className="text-[10.5px] text-fg-subtle font-mono shrink-0">{resource.handle.id}</span>
                        </RefChip>
                      )
                    })}
                  </section>
                ))}
              </div>
            </>
          )
        : call.result !== undefined && (
          <div className="p-3 flex flex-col gap-1 overflow-auto">
            <div className="panel-title">Result</div>
            <div className="font-mono"><ValueView value={call.result} /></div>
          </div>
        )}
    </div>
  )
}

function findNode(node: AxNode, path: string): AxNode | undefined {
  if (node.path === path)
    return node
  for (const child of node.children) {
    const hit = findNode(child, path)
    if (hit)
      return hit
  }
  return undefined
}

/** Groups window handles by app (first-seen order); other handles stay in one ungrouped run. */
function groupByApp(refs: string[], resources: Record<string, Resource>): RefGroup[] {
  const groups = new Map<string, RefGroup>()
  for (const ref of refs) {
    const resource = resources[ref]
    const key = resource?.kind === 'window' ? `app:${resource.handle.bundleId ?? resource.handle.app ?? ''}` : 'other'
    let group = groups.get(key)
    if (!group) {
      group = resource?.kind === 'window'
        ? { app: resource.handle.app ?? '', bundleId: resource.handle.bundleId, key, refs: [] }
        : { key, refs: [] }
      groups.set(key, group)
    }
    group.refs.push(ref)
  }
  return [...groups.values()]
}

function HandlePanel() {
  const selectedRef = usePlayground(state => state.selectedRef)
  const resource = usePlayground(state => (state.selectedRef ? state.resources[state.selectedRef] : undefined))
  const calls = usePlayground(state => state.calls)
  const binds = usePlayground(state => state.binds)
  const producer = resource?.callId === undefined ? undefined : calls.find(call => call.id === resource.callId)
  const consumers = useMemo(() => (selectedRef ? consumersOf(calls, selectedRef) : []), [selectedRef, calls])
  const bindings = useMemo(() => (selectedRef ? bindingsOf(binds, selectedRef) : []), [selectedRef, binds])

  if (!selectedRef || !resource)
    return <Empty>Select a ◆ handle in the calls, console, variables or a pinned card.</Empty>

  return (
    <div className="text-[12.5px] p-3 flex flex-col gap-3 h-full overflow-auto">
      <div className="flex gap-2 items-center">
        <RefChip refId={selectedRef} />
        <button
          className="text-fg-subtle ml-auto rounded flex size-6 cursor-pointer items-center justify-center hover:(text-accent bg-surface-2)"
          onClick={event => actions.addPin({ label: selectedRef, ref: selectedRef, seq: resource.seq, x: event.clientX - 300, y: event.clientY + 12 })}
          title="Pin as a floating card"
          type="button"
        >
          <span className="i-ph-push-pin text-[13px]" />
        </button>
      </div>
      <ResourcePreview resource={resource} />

      <div className="flex flex-col gap-1.5">
        <div className="panel-title">Lineage</div>
        {producer && (
          <LineageRow
            label="produced by"
            seq={producer.endSeq ?? producer.seq}
            text={`${producer.method}${producer.line === null ? '' : ` · L${producer.line} #${producer.hit}`}`}
          />
        )}
        {bindings.map(binding => (
          <LineageRow key={`${binding.seq}-${binding.name}`} label="bound to" seq={binding.seq} text={`${binding.name} · L${binding.line} #${binding.hit}`} />
        ))}
        {consumers.map(call => (
          <LineageRow key={call.id} label="used by" seq={call.seq} text={`${call.method} · L${call.line ?? '?'} #${call.hit}`} />
        ))}
        {!producer && bindings.length === 0 && consumers.length === 0 && <span className="text-fg-subtle">No recorded lineage in this run.</span>}
        {/* TODO(value-provenance): derived plain values (e.g. `centerOf(match.bounds)`)
            lose their link to the handle. Tagging derived values in the worker
            would let a click point trace back to the OCR match it came from. */}
      </div>

      <details className="text-[12px]">
        <summary className="text-fg-subtle cursor-pointer">handle</summary>
        <div className="font-mono mt-1.5">
          <ValueView value={resource.handle} />
        </div>
      </details>
    </div>
  )
}

function LineageRow({ label, seq, text }: { label: string, seq: number, text: string }) {
  return (
    <button
      className="px-1.5 py-1 text-left rounded flex gap-2 cursor-pointer items-center hover:bg-surface-2"
      onClick={() => actions.setCursor(seq)}
      title="Move the time cursor here"
      type="button"
    >
      <span className="text-[11px] text-fg-subtle shrink-0 w-20">{label}</span>
      <span className="text-fg font-mono truncate">{text}</span>
      <span className="text-[11px] text-fg-subtle font-mono ml-auto shrink-0">{`seq ${seq}`}</span>
    </button>
  )
}
