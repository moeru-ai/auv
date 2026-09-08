import AppKit
import CoreGraphics
import Foundation

private func stampKeyboardTarget(_ event: CGEvent, pid: Int64, windowNumber: Int64) {
  event.setIntegerValueField(.eventTargetUnixProcessID, value: pid)
  // Application-scoped delivery addresses only the process, not a fabricated window.
  if windowNumber == 0 { return }
  event.setIntegerValueField(.mouseEventWindowUnderMousePointer, value: windowNumber)
  event.setIntegerValueField(.mouseEventWindowUnderMousePointerThatCanHandleThisEvent, value: windowNumber)
  if let eventWindowNumber = CGEventField(rawValue: 51) {
    event.setIntegerValueField(eventWindowNumber, value: windowNumber)
  }
  if let eventWindowId = CGEventField(rawValue: 40) {
    event.setIntegerValueField(eventWindowId, value: windowNumber)
  }
}

private enum KeyboardDelivery {
  case foreground
  case process(pid: Int64, windowNumber: Int64)

  var label: String {
    switch self {
    case .foreground:
      return "foreground"
    case .process:
      return "window-targeted"
    }
  }

  var eventSource: CGEventSource? {
    switch self {
    case .foreground:
      // Sunshine uses a private source and the session tap for foreground
      // keyboard delivery. Keep this separate from PID-targeted delivery,
      // whose event state is intentionally derived from the HID system.
      // https://github.com/LizardByte/Sunshine/blob/25c06d79b54f3d092d3fedd5f5ba44989f394692/src/platform/macos/input.cpp#L329-L375
      return CGEventSource(stateID: .privateState)
    case .process:
      return CGEventSource(stateID: .hidSystemState)
    }
  }

  func post(_ event: CGEvent) {
    switch self {
    case .foreground:
      event.post(tap: .cgSessionEventTap)
    case let .process(pid, windowNumber):
      stampKeyboardTarget(event, pid: pid, windowNumber: windowNumber)
      event.postToPid(pid_t(pid))
    }
  }
}

private struct ModifierKey {
  let keyCode: CGKeyCode
  let flag: CGEventFlags
}

private func modifierKeys(
  command: Bool,
  shift: Bool,
  option: Bool,
  control: Bool
) -> [ModifierKey] {
  var keys: [ModifierKey] = []
  if command {
    keys.append(ModifierKey(keyCode: 55, flag: .maskCommand))
  }
  if shift {
    keys.append(ModifierKey(keyCode: 56, flag: .maskShift))
  }
  if option {
    keys.append(ModifierKey(keyCode: 58, flag: .maskAlternate))
  }
  if control {
    keys.append(ModifierKey(keyCode: 59, flag: .maskControl))
  }
  return keys
}

private func validatedKeyCode(_ keyCode: Int32) -> CGKeyCode? {
  guard keyCode >= 0 && keyCode <= Int32(UInt16.max) else {
    return nil
  }
  return CGKeyCode(UInt16(keyCode))
}

private func makeKeyboardEvent(
  source: CGEventSource?,
  keyCode: CGKeyCode,
  keyDown: Bool,
  flags: CGEventFlags = []
) -> CGEvent? {
  let event = CGEvent(keyboardEventSource: source, virtualKey: keyCode, keyDown: keyDown)
  event?.flags = flags
  return event
}

private func typeText(
  delivery: KeyboardDelivery,
  text: String,
  inter_char_delay_ms: UInt64
) -> NativeActionResponse {
  let source = delivery.eventSource
  let delaySeconds = Double(inter_char_delay_ms) / 1000.0
  let characters = Array(text)

  for (index, character) in characters.enumerated() {
    guard
      let down = makeKeyboardEvent(source: source, keyCode: 0, keyDown: true),
      let up = makeKeyboardEvent(source: source, keyCode: 0, keyDown: false)
    else {
      return nativeActionError(
        "failed to create \(delivery.label) keyboard event",
        "grant Accessibility permission and retry"
      )
    }

    let utf16 = Array(String(character).utf16)
    utf16.withUnsafeBufferPointer { buffer in
      down.keyboardSetUnicodeString(stringLength: buffer.count, unicodeString: buffer.baseAddress)
      up.keyboardSetUnicodeString(stringLength: buffer.count, unicodeString: buffer.baseAddress)
    }

    // Provenance: CUA keyboard and KWWK keyboard background dispatch patterns.
    // https://github.com/trycua/cua/blob/a3448588286b6373013a5fa9072ac8bafb6681d6/libs/cua-driver-rs/crates/platform-macos/src/input/keyboard.rs#L35-L90
    // https://github.com/EYHN/kwwk-computer-use-core/blob/eddd9e5475095de58bcb81cafbad79d1f5c5495d/Sources/KWWKComputerUseCore/BackgroundInputDispatcher.swift#L264-L333
    delivery.post(down)
    delivery.post(up)

    if index < characters.count - 1 && delaySeconds > 0 {
      Thread.sleep(forTimeInterval: delaySeconds)
    }
  }

  return nativeActionOk()
}

// Existing shortcut callers (including clipboard transactions) use the same
// key combination event builder as explicit multi-key input. Preserve modifier order.
private func hotkey(
  delivery: KeyboardDelivery,
  keyCode: Int32,
  command: Bool,
  shift: Bool,
  option: Bool,
  control: Bool
) -> NativeActionResponse {
  let codes = modifierKeys(command: command, shift: shift, option: option, control: control)
    .map { Int32($0.keyCode) } + [keyCode]
  return pressKeys(delivery: delivery, keyCodes: codes)
}

// A key combination prepares the complete event list before delivery. Failure to create
// any event therefore cannot leave a modifier down. Posting itself has no OS
// acknowledgement; a completed key combination is still unverified input submission.
private func pressKeys(delivery: KeyboardDelivery, keyCodes: [Int32]) -> NativeActionResponse {
  guard !keyCodes.isEmpty else { return nativeActionError("keys must not be empty", "provide at least one key") }
  let source = delivery.eventSource
  var events: [CGEvent] = []
  var flags = CGEventFlags()
  let modifiers = modifierKeys(command: true, shift: true, option: true, control: true)
  func modifier(_ code: Int32) -> CGEventFlags {
    modifiers.first { Int32($0.keyCode) == code }?.flag ?? []
  }
  for (codes, down) in [(keyCodes, true), (Array(keyCodes.reversed()), false)] {
    for code in codes {
      if down { flags.formUnion(modifier(code)) } else { flags.subtract(modifier(code)) }
      guard let key = validatedKeyCode(code),
        let event = makeKeyboardEvent(source: source, keyCode: key, keyDown: down, flags: flags) else {
        return nativeActionError("failed to create key combination event", "check key codes and Accessibility permission")
      }
      events.append(event)
    }
  }
  for event in events { delivery.post(event) }
  return nativeActionOk()
}

func press_keys_foreground(key_codes: RustVec<Int32>) -> NativeActionResponse {
  pressKeys(delivery: .foreground, keyCodes: Array(key_codes))
}

func press_keys_in_window(pid: Int64, window_number: Int64, key_codes: RustVec<Int32>) -> NativeActionResponse {
  pressKeys(delivery: .process(pid: pid, windowNumber: window_number), keyCodes: Array(key_codes))
}

func type_text_foreground(text: RustString, inter_char_delay_ms: UInt64) -> NativeActionResponse {
  typeText(delivery: .foreground, text: text.toString(), inter_char_delay_ms: inter_char_delay_ms)
}

func press_key_foreground(key_code: Int32) -> NativeActionResponse {
  pressKeys(delivery: .foreground, keyCodes: [key_code])
}

func hotkey_foreground(
  key_code: Int32,
  command: Bool,
  shift: Bool,
  option: Bool,
  control: Bool
) -> NativeActionResponse {
  hotkey(
    delivery: .foreground,
    keyCode: key_code,
    command: command,
    shift: shift,
    option: option,
    control: control
  )
}

func type_text_in_window(
  pid: Int64,
  window_number: Int64,
  text: RustString,
  inter_char_delay_ms: UInt64
) -> NativeActionResponse {
  typeText(
    delivery: .process(pid: pid, windowNumber: window_number),
    text: text.toString(),
    inter_char_delay_ms: inter_char_delay_ms
  )
}

func press_key_in_window(pid: Int64, window_number: Int64, key_code: Int32) -> NativeActionResponse {
  pressKeys(
    delivery: .process(pid: pid, windowNumber: window_number),
    keyCodes: [key_code]
  )
}

func hotkey_in_window(
  pid: Int64,
  window_number: Int64,
  key_code: Int32,
  command: Bool,
  shift: Bool,
  option: Bool,
  control: Bool
) -> NativeActionResponse {
  hotkey(
    delivery: .process(pid: pid, windowNumber: window_number),
    keyCode: key_code,
    command: command,
    shift: shift,
    option: option,
    control: control
  )
}
