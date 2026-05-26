import AppKit
import CoreGraphics
import Foundation

private func keyboardEventSource() -> CGEventSource? {
  CGEventSource(stateID: .hidSystemState)
}

private func stampKeyboardTarget(_ event: CGEvent, pid: Int64, windowNumber: Int64) {
  event.setIntegerValueField(.eventTargetUnixProcessID, value: pid)
  event.setIntegerValueField(.mouseEventWindowUnderMousePointer, value: windowNumber)
  event.setIntegerValueField(.mouseEventWindowUnderMousePointerThatCanHandleThisEvent, value: windowNumber)
  if let eventWindowNumber = CGEventField(rawValue: 51) {
    event.setIntegerValueField(eventWindowNumber, value: windowNumber)
  }
  if let eventWindowId = CGEventField(rawValue: 40) {
    event.setIntegerValueField(eventWindowId, value: windowNumber)
  }
}

private func postKeyboardEvent(_ event: CGEvent, pid: Int64, windowNumber: Int64) {
  stampKeyboardTarget(event, pid: pid, windowNumber: windowNumber)
  event.postToPid(pid_t(pid))
}

private func modifierFlags(
  command: Bool,
  shift: Bool,
  option: Bool,
  control: Bool
) -> CGEventFlags {
  var flags = CGEventFlags()
  if command {
    flags.insert(.maskCommand)
  }
  if shift {
    flags.insert(.maskShift)
  }
  if option {
    flags.insert(.maskAlternate)
  }
  if control {
    flags.insert(.maskControl)
  }
  return flags
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

func type_text_in_window(
  pid: Int64,
  window_number: Int64,
  text: RustString,
  inter_char_delay_ms: UInt64
) -> NativeActionResponse {
  let source = keyboardEventSource()
  let delaySeconds = Double(inter_char_delay_ms) / 1000.0
  let characters = Array(text.toString())

  for (index, character) in characters.enumerated() {
    guard
      let down = makeKeyboardEvent(source: source, keyCode: 0, keyDown: true),
      let up = makeKeyboardEvent(source: source, keyCode: 0, keyDown: false)
    else {
      return nativeActionError(
        "failed to create window-targeted keyboard event",
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
    postKeyboardEvent(down, pid: pid, windowNumber: window_number)
    postKeyboardEvent(up, pid: pid, windowNumber: window_number)

    if index < characters.count - 1 && delaySeconds > 0 {
      Thread.sleep(forTimeInterval: delaySeconds)
    }
  }

  return nativeActionOk()
}

func press_key_in_window(pid: Int64, window_number: Int64, key_code: Int32) -> NativeActionResponse {
  let source = keyboardEventSource()
  guard let virtualKey = validatedKeyCode(key_code) else {
    return nativeActionError(
      "invalid key_code \(key_code)",
      "pass a key_code between 0 and \(UInt16.max)"
    )
  }
  guard
    let down = makeKeyboardEvent(source: source, keyCode: virtualKey, keyDown: true),
    let up = makeKeyboardEvent(source: source, keyCode: virtualKey, keyDown: false)
  else {
    return nativeActionError(
      "failed to create window-targeted key press event",
      "grant Accessibility permission and retry"
    )
  }

  postKeyboardEvent(down, pid: pid, windowNumber: window_number)
  postKeyboardEvent(up, pid: pid, windowNumber: window_number)
  return nativeActionOk()
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
  let source = keyboardEventSource()
  guard let virtualKey = validatedKeyCode(key_code) else {
    return nativeActionError(
      "invalid key_code \(key_code)",
      "pass a key_code between 0 and \(UInt16.max)"
    )
  }
  let fullFlags = modifierFlags(command: command, shift: shift, option: option, control: control)
  let modifiers = modifierKeys(command: command, shift: shift, option: option, control: control)
  var events: [CGEvent] = []
  var currentFlags = CGEventFlags()

  for modifier in modifiers {
    currentFlags.insert(modifier.flag)
    guard
      let event = makeKeyboardEvent(
        source: source,
        keyCode: modifier.keyCode,
        keyDown: true,
        flags: currentFlags
      )
    else {
      return nativeActionError(
        "failed to create window-targeted modifier key event",
        "grant Accessibility permission and retry"
      )
    }
    events.append(event)
  }

  guard
    let down = makeKeyboardEvent(source: source, keyCode: virtualKey, keyDown: true, flags: fullFlags),
    let up = makeKeyboardEvent(source: source, keyCode: virtualKey, keyDown: false, flags: fullFlags)
  else {
    return nativeActionError(
      "failed to create window-targeted hotkey event",
      "grant Accessibility permission and retry"
    )
  }
  events.append(down)
  events.append(up)

  for modifier in modifiers.reversed() {
    currentFlags.remove(modifier.flag)
    guard
      let event = makeKeyboardEvent(
        source: source,
        keyCode: modifier.keyCode,
        keyDown: false,
        flags: currentFlags
      )
    else {
      return nativeActionError(
        "failed to create window-targeted modifier key event",
        "grant Accessibility permission and retry"
      )
    }
    events.append(event)
  }

  for event in events {
    postKeyboardEvent(event, pid: pid, windowNumber: window_number)
  }

  return nativeActionOk()
}
