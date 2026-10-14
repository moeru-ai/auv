import { describe, expect, it } from 'vitest'

import { terminalUseOf } from './terminalUse'

const invoke = (call: Record<string, unknown>, output = '{"status":"completed"}') => ({ arguments: JSON.stringify(call), output, tool: 'invoke' })

describe('terminalUseOf', () => {
  // ROOT CAUSE:
  //
  // In the 2026-10-12 run, gpt-5.5 solved GIMP and Calc tasks by opening GNOME
  // Terminal and pasting `python3 - <<'PY'` programs (gimp/2e6f678f), so AUV
  // only typed text. Summaries counted those episodes like GUI work.
  //
  // The classifier marks terminal and script steps so summaries can split them.
  it('counts a terminal opened by shortcut and a pasted here-document script', () => {
    const steps = [
      invoke({ command_id: 'display.capture', inputs: {} }),
      invoke({ command_id: 'input.key', inputs: { key: 'Ctrl+Alt+T' } }),
      invoke({ command_id: 'input.pasteText', inputs: { text: 'python3 - <<\'PY\'\nimport glob\nPY\n' } }),
      invoke({ command_id: 'input.clickPoint', inputs: { x: '10', y: '20' } }),
    ]
    expect(terminalUseOf(steps)).toEqual({ scriptSteps: 1, terminalSteps: 2 })
  })

  it('counts calls that target a terminal application and keyboard batches that type a one-liner', () => {
    const steps = [
      invoke({ command_id: 'input.key', inputs: { key: 'Return' }, target: { application_id: 'org.gnome.Terminal' } }),
      invoke({ command_id: 'input.keyboard', inputs: { actions: JSON.stringify([{ kind: 'type_text', text: 'python3 -c "print(1)"' }, { keys: ['return'], kind: 'press' }]) } }),
    ]
    expect(terminalUseOf(steps)).toEqual({ scriptSteps: 1, terminalSteps: 2 })
  })

  it('counts a paste that the X11 driver delivered into a terminal emulator', () => {
    const output = '{"result":{"attempts":[{"message":"pasted with Ctrl+Shift+V because the active window is the terminal emulator org.gnome.Terminal"}]}}'
    expect(terminalUseOf([invoke({ command_id: 'input.pasteText', inputs: { text: 'ls' } }, output)])).toEqual({ scriptSteps: 0, terminalSteps: 1 })
  })

  it('leaves GUI work, plain text, and episode tools uncounted', () => {
    const steps = [
      invoke({ command_id: 'input.pasteText', inputs: { text: 'Keira Daily\n50 Dollar Dollar' }, target: { application_id: 'libreoffice-calc' } }),
      invoke({ command_id: 'input.typeText', inputs: { text: 'a << b is a shift' } }),
      invoke({ command_id: 'input.key', inputs: { key: 'ctrl+alt+delete' } }),
      { tool: 'done' },
      { arguments: 'not json', tool: 'invoke' },
    ]
    // `<<` is a here-document only with a closing delimiter line.
    expect(terminalUseOf(steps)).toEqual({ scriptSteps: 0, terminalSteps: 0 })
  })
})
