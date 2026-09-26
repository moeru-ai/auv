const input = document.querySelector('textarea')!
document.title = new URL(location.href).searchParams.get('title') ?? 'AUV keyboard receiver'

let caseId: null | string = null
let events: Array<Record<string, unknown>> = []
let sequence = 0
let polling = false

function receipt() {
  return {
    caseId,
    documentFocused: document.hasFocus(),
    events,
    inputSelected: document.activeElement === input,
    selection: [input.selectionStart, input.selectionEnd],
    sequence: ++sequence,
    userAgent: navigator.userAgent,
    value: input.value,
  }
}

function send() {
  fetch('/receipt', {
    body: JSON.stringify(receipt()),
    headers: { 'Content-Type': 'application/json' },
    method: 'POST',
  }).catch(() => {})
}

for (const name of ['keydown', 'keyup', 'beforeinput', 'input', 'compositionstart', 'compositionupdate', 'compositionend']) {
  input.addEventListener(name, (received) => {
    const event = received as Event & Partial<InputEvent & KeyboardEvent>

    events.push({
      alt: event.altKey,
      code: event.code,
      composing: event.isComposing,
      control: event.ctrlKey,
      data: event.data,
      documentFocused: document.hasFocus(),
      inputType: event.inputType,
      key: event.key,
      meta: event.metaKey,
      receivedMs: performance.now(),
      repeat: event.repeat,
      shift: event.shiftKey,
      trusted: event.isTrusted,
      type: event.type,
    })

    send()
  })
}

// Fixture setup resets state without synthesizing any keyboard event. The
// only tested input comes from AUV through the native OS input path.
setInterval(async () => {
  if (polling)
    return

  polling = true

  try {
    const command = await (await fetch('/command', { cache: 'no-store' })).json()

    if (command.caseId !== caseId) {
      // Finish any prior IME composition before resetting the next case.
      input.blur()

      caseId = command.caseId
      input.value = command.initial
      input.focus()
      input.setSelectionRange(input.value.length, input.value.length)
      events = []
      document.querySelector('#case')!.textContent = caseId

      send()
    }
  }
  finally {
    polling = false
  }
}, 100)

input.focus()
send()

// Focus experiments also observe transitions without keyboard/DOM input.
if (new URL(location.href).searchParams.has('observeFocus')) {
  let lastFocus: boolean | undefined

  setInterval(() => {
    const focused = document.hasFocus()

    if (focused !== lastFocus) {
      lastFocus = focused
      send()
    }
  }, 10)
}
