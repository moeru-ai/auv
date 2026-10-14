import type { AgentStep } from './agent'

/**
 * How much of an episode went through a terminal instead of the task's GUI.
 *
 * OSWorld accepts any route to the final state, but this eval measures AUV
 * driving applications. A model that opens a terminal and pastes a script
 * (`python3 - <<'PY'` with openpyxl or ImageMagick) uses AUV only to type, so
 * summaries report GUI-only episodes separately from terminal ones.
 */
export interface TerminalUse {
  /** Text inputs that carried a script or a one-line interpreter program. */
  scriptSteps: number
  /** Steps that targeted, opened, or typed into a terminal emulator. */
  terminalSteps: number
}

// NOTICE: Reads calls and AUV replies, not screenshots: a terminal reached
// without a terminal target, Ctrl+Alt+T, or a script is missed. Calibrated on
// the 2026-10-12 gpt-5.5 run (22 Ctrl+Alt+T, ~250 `org.gnome.Terminal`
// targets, 113 script pastes); extend only with evidence.
const terminalApplication = /terminal|xterm|konsole|tilix|terminator|kitty|alacritty|wezterm/i
const hereDocumentOpener = /<<-?\s*['"]?(\w+)['"]?/
const scriptText = [
  // Interpreter one-liners and stdin programs
  /\b(?:python3?|perl|ruby|node)\s+(?:-c|-e|-)(?:\s|$)/,
  /\b(?:bash|sh|zsh)\s+-c\b/,
  /^#!\//m,
]

interface InvokeCall {
  command_id?: string
  inputs?: Record<string, unknown>
  target?: { application_id?: string }
}

/** Classifies every step of one episode. */
export function terminalUseOf(steps: AgentStep[]): TerminalUse {
  let terminalSteps = 0
  let scriptSteps = 0
  for (const step of steps) {
    const call = parseCall(step.arguments)
    const texts = call == null ? [] : textsOf(call.inputs)
    const script = texts.some(text => isHereDocument(text) || scriptText.some(pattern => pattern.test(text)))
    const terminal = script
      || terminalApplication.test(call?.target?.application_id ?? '')
      || keysOf(call?.inputs).includes('ctrl+alt+t')
      // The X11 driver names the terminal it pasted into with Ctrl+Shift+V.
      || (step.output?.includes('because the active window is the terminal emulator') ?? false)
    terminalSteps += Number(terminal)
    scriptSteps += Number(script)
  }
  return { scriptSteps, terminalSteps }
}

/** `python3 - <<'PY'` … `PY`: an opener whose delimiter closes a later line. */
function isHereDocument(text: string): boolean {
  const lines = text.split('\n')
  const index = lines.findIndex(line => hereDocumentOpener.test(line))
  if (index === -1) {
    return false
  }
  const delimiter = hereDocumentOpener.exec(lines[index])?.[1]
  return lines.slice(index + 1).some(line => line.trim() === delimiter)
}

function keyboardActions(raw: unknown): Array<Record<string, unknown>> {
  let value = raw
  if (typeof raw === 'string') {
    try {
      value = JSON.parse(raw)
    }
    catch {
      return []
    }
  }
  return Array.isArray(value) ? value.filter((action): action is Record<string, unknown> => typeof action === 'object' && action != null) : []
}

/** Key chords a call presses, normalized to lowercase `a+b+c` without spaces. */
function keysOf(inputs: Record<string, unknown> | undefined): string[] {
  if (inputs == null) {
    return []
  }
  const chords: string[] = []
  const add = (value: unknown): void => {
    if (typeof value === 'string') {
      chords.push(value)
    }
    else if (Array.isArray(value) && value.every(key => typeof key === 'string')) {
      chords.push(value.join('+'))
    }
  }
  add(inputs.key)
  add(inputs.keys)
  for (const action of keyboardActions(inputs.actions)) {
    add(action.keys)
  }
  return chords.map(chord => chord.toLowerCase().replaceAll(/\s+/g, ''))
}

function parseCall(raw: string | undefined): InvokeCall | undefined {
  if (raw == null) {
    return undefined
  }
  try {
    const value: unknown = JSON.parse(raw)
    return typeof value === 'object' && value != null ? value as InvokeCall : undefined
  }
  catch {
    return undefined
  }
}

/** Text that a call types or pastes, including `input.keyboard` actions, which arrive as a JSON string. */
function textsOf(inputs: Record<string, unknown> | undefined): string[] {
  if (inputs == null) {
    return []
  }
  const texts = typeof inputs.text === 'string' ? [inputs.text] : []
  for (const action of keyboardActions(inputs.actions)) {
    if (typeof action.text === 'string') {
      texts.push(action.text)
    }
  }
  return texts
}
