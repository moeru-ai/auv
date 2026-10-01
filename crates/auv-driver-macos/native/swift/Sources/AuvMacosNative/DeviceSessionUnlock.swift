import AppKit
import ApplicationServices
import CoreGraphics
import Darwin
import Foundation
import IOKit
import IOKit.pwr_mgt

// These routes belong only in the signed graphical helper for the selected
// physical console. The caller independently checks the same session.
private func selectedConsoleLockState(uid: UInt32, selector: String) -> Bool? {
  guard selector.hasPrefix("macos:"), selector.count == 42 else { return nil }
  let root: io_registry_entry_t
  if #available(macOS 12.0, *) {
    root = IORegistryGetRootEntry(kIOMainPortDefault)
  } else {
    return nil
  }
  guard root != 0 else { return nil }
  defer { IOObjectRelease(root) }
  guard let locked = IORegistryEntryCreateCFProperty(root, "IOConsoleLocked" as CFString, kCFAllocatorDefault, 0)?.takeRetainedValue(),
    let usersValue = IORegistryEntryCreateCFProperty(root, "IOConsoleUsers" as CFString, kCFAllocatorDefault, 0)?.takeRetainedValue(),
    let isLocked = locked as? Bool,
    let users = usersValue as? [[String: Any]]
  else { return nil }
  let active = users.filter {
    $0["kCGSSessionOnConsoleKey"] as? Bool == true &&
      $0["kCGSessionLoginDoneKey"] as? Bool == true
  }
  guard active.count == 1, let user = active.first,
    let name = user["kCGSSessionUserNameKey"] as? String,
    !name.isEmpty, name != "loginwindow",
    let observedUid = user["kCGSSessionUserIDKey"] as? NSNumber,
    observedUid.int64Value == Int64(uid),
    let uuid = user["CGSSessionUniqueSessionUUID"] as? String,
    "macos:\(uuid)" == selector
  else { return nil }
  return isLocked
}

private func selectedConsoleIsLocked(uid: UInt32, selector: String) -> Bool {
  selectedConsoleLockState(uid: uid, selector: selector) == true
}

private func axValue(_ element: AXUIElement, _ attribute: String) -> CFTypeRef? {
  var value: CFTypeRef?
  guard AXUIElementCopyAttributeValue(element, attribute as CFString, &value) == .success else { return nil }
  return value
}

private func axBoolean(_ element: AXUIElement, _ attribute: String) -> Bool {
  guard let value = axValue(element, attribute), CFGetTypeID(value) == CFBooleanGetTypeID() else { return false }
  return CFBooleanGetValue((value as! CFBoolean))
}

private func axElement(_ element: AXUIElement, _ attribute: String) -> AXUIElement? {
  guard let value = axValue(element, attribute), CFGetTypeID(value) == AXUIElementGetTypeID() else { return nil }
  return unsafeBitCast(value, to: AXUIElement.self)
}

private func axChildren(_ element: AXUIElement) -> [AXUIElement]? {
  var value: CFTypeRef?
  let result = AXUIElementCopyAttributeValue(element, kAXChildrenAttribute as CFString, &value)
  // NOTICE: AXError.h defines kAXErrorNoValue (-25212); the locked loginwindow's
  // AXButton returned it for AXChildren while the focused secure field was present.
  if result == .attributeUnsupported || result == .noValue { return [] }
  guard result == .success, let value, let array = value as? NSArray else { return nil }
  var children: [AXUIElement] = []
  for item in array {
    let raw = item as CFTypeRef
    guard CFGetTypeID(raw) == AXUIElementGetTypeID() else { return nil }
    children.append(unsafeBitCast(raw, to: AXUIElement.self))
  }
  return children
}

private func focusedLoginwindowSecureField() -> (pid_t, AXUIElement)? {
  let apps = NSRunningApplication.runningApplications(withBundleIdentifier: "com.apple.loginwindow")
    .filter { !$0.isTerminated }
  guard apps.count == 1, let app = apps.first else { return nil }
  let pid = app.processIdentifier
  let root = AXUIElementCreateApplication(pid)
  guard let focused = axElement(root, kAXFocusedUIElementAttribute as String) else { return nil }
  var focusedPid: pid_t = 0
  guard AXUIElementGetPid(focused, &focusedPid) == .success, focusedPid == pid else { return nil }
  var pending = [root]
  var seen: [AXUIElement] = []
  var secureFields: [AXUIElement] = []
  // A malformed or changing tree must fail closed instead of hiding a second field.
  while let current = pending.popLast() {
    if seen.contains(where: { CFEqual($0, current) }) { continue }
    seen.append(current)
    guard seen.count <= 512 else { return nil }
    var roleValue: CFTypeRef?
    guard AXUIElementCopyAttributeValue(current, kAXRoleAttribute as CFString, &roleValue) == .success,
      let role = roleValue as? String
    else { return nil }
    if role == kAXTextFieldRole as String {
      var subroleValue: CFTypeRef?
      let subroleResult = AXUIElementCopyAttributeValue(current, kAXSubroleAttribute as CFString, &subroleValue)
      if subroleResult != .attributeUnsupported && subroleResult != .noValue {
        guard subroleResult == .success, let subrole = subroleValue as? String else { return nil }
        if subrole == kAXSecureTextFieldSubrole as String {
          var enabledValue: CFTypeRef?
          guard AXUIElementCopyAttributeValue(current, kAXEnabledAttribute as CFString, &enabledValue) == .success,
            let enabledValue, CFGetTypeID(enabledValue) == CFBooleanGetTypeID()
          else { return nil }
          if CFBooleanGetValue((enabledValue as! CFBoolean)) {
            secureFields.append(current)
          }
        }
      }
    }
    guard let children = axChildren(current) else { return nil }
    pending.append(contentsOf: children)
  }
  guard secureFields.count == 1, let field = secureFields.first,
    CFEqual(field, focused),
    axBoolean(field, kAXFocusedAttribute as String)
  else { return nil }
  return (pid, field)
}

private func sameLockedField(uid: UInt32, selector: String, pid: pid_t, field: AXUIElement) -> Bool {
  guard selectedConsoleIsLocked(uid: uid, selector: selector),
    let (currentPid, currentField) = focusedLoginwindowSecureField()
  else { return false }
  return currentPid == pid && CFEqual(currentField, field)
}

private func unicodePair(_ value: String) -> (CGEvent, CGEvent)? {
  guard let down = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: true),
    let up = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: false)
  else { return nil }
  let units = Array(value.utf16)
  units.withUnsafeBufferPointer { buffer in
    down.keyboardSetUnicodeString(stringLength: buffer.count, unicodeString: buffer.baseAddress)
    up.keyboardSetUnicodeString(stringLength: buffer.count, unicodeString: buffer.baseAddress)
  }
  return (down, up)
}

private func keyPair(_ code: CGKeyCode, flags: CGEventFlags = []) -> (CGEvent, CGEvent)? {
  guard let down = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: true),
    let up = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: false)
  else { return nil }
  down.flags = flags
  up.flags = flags
  return (down, up)
}

private func post(_ pair: (CGEvent, CGEvent)) {
  pair.0.post(tap: .cghidEventTap)
  pair.1.post(tap: .cghidEventTap)
}

func submit_locked_session_credential(
  credential: RustVec<UInt8>,
  expected_uid: UInt32,
  selector: RustString,
  posting_budget_seconds: Double
) -> NativeDeviceUnlockOutcome {
  guard posting_budget_seconds.isFinite, posting_budget_seconds > 0, posting_budget_seconds <= 7 else {
    return .DeadlineExceeded
  }
  let deadline = ProcessInfo.processInfo.systemUptime + posting_budget_seconds
  guard getuid() == expected_uid, geteuid() == expected_uid else {
    return .IdentityMismatch
  }
  guard AXIsProcessTrusted(), CGPreflightPostEventAccess() else {
    return .PermissionMissing
  }
  let selected = selector.toString()
  guard selectedConsoleIsLocked(uid: expected_uid, selector: selected) else {
    return .SessionChanged
  }

  var bytes = Array(credential)
  defer {
    _ = bytes.withUnsafeMutableBytes { raw in
      raw.initializeMemory(as: UInt8.self, repeating: 0)
    }
  }
  // NOTICE: Bound event allocation for malformed enrollment data; a larger
  // credential needs a separately reviewed delivery and memory limit.
  guard !bytes.isEmpty, bytes.count <= 1024,
    let secret = String(bytes: bytes, encoding: .utf8),
    !secret.unicodeScalars.contains(where: { $0.value < 0x20 || $0.value == 0x7f }),
    let activation = unicodePair("2"),
    let command = keyPair(55),
    let selectAll = keyPair(0, flags: .maskCommand),
    let clear = keyPair(51),
    let submit = keyPair(36)
  else {
    return .InvalidRequest
  }

  // Allocate all events before posting any input. Posting has no recipient acknowledgement.
  var characterEvents: [(CGEvent, CGEvent)] = []
  for character in secret {
    guard let pair = unicodePair(String(character)) else {
      return .EventUnavailable
    }
    characterEvents.append(pair)
  }

  guard ProcessInfo.processInfo.systemUptime < deadline else {
    return .DeadlineExceeded
  }
  var wakeAssertion: IOPMAssertionID?
  defer {
    if let wakeAssertion { _ = IOPMAssertionRelease(wakeAssertion) }
  }
  if CGDisplayIsAsleep(CGMainDisplayID()) != 0 {
    // NOTICE: IOPMLib.h documents that a display-sleep prevention assertion
    // cannot light an already-off display; IOPMAssertionDeclareUserActivity
    // can. The installed helper failed to unlock the reachable locked spare Mac
    // when its display slept; a separate non-secret probe woke that display.
    // Remove this wake gate only if the installed helper proves input delivery
    // with a sleeping display without it.
    var assertion: IOPMAssertionID = 0
    guard IOPMAssertionDeclareUserActivity(
      "AUV locked session unlock" as CFString,
      kIOPMUserActiveRemote,
      &assertion
    ) == kIOReturnSuccess else {
      return .WakeUnavailable
    }
    wakeAssertion = assertion
    let wakeDeadline = min(deadline, ProcessInfo.processInfo.systemUptime + 2)
    while CGDisplayIsAsleep(CGMainDisplayID()) != 0 {
      guard ProcessInfo.processInfo.systemUptime < wakeDeadline,
        selectedConsoleIsLocked(uid: expected_uid, selector: selected)
      else {
        return .WakeUnavailable
      }
      Thread.sleep(forTimeInterval: 0.05)
    }
  }
  guard selectedConsoleIsLocked(uid: expected_uid, selector: selected),
    ProcessInfo.processInfo.systemUptime < deadline
  else {
    return .SessionChanged
  }
  // NOTICE(device-entry-focus-after-wake): An installed paired attempt woke
  // the display but returned FocusUnavailable after the fixed 350 ms check.
  // Retry only the harmless activation character while the same session stays
  // locked. Remove this bounded wait if a future installed gate establishes a
  // reliable loginwindow readiness signal.
  let focusDeadline = min(deadline, ProcessInfo.processInfo.systemUptime + 2)
  var nextActivation = ProcessInfo.processInfo.systemUptime
  var focusedField: (pid_t, AXUIElement)?
  while ProcessInfo.processInfo.systemUptime < focusDeadline {
    guard selectedConsoleIsLocked(uid: expected_uid, selector: selected) else {
      return .SessionChanged
    }
    if ProcessInfo.processInfo.systemUptime >= nextActivation {
      post(activation)
      nextActivation = ProcessInfo.processInfo.systemUptime + 0.35
    }
    Thread.sleep(forTimeInterval: 0.05)
    if let candidate = focusedLoginwindowSecureField() {
      focusedField = candidate
      break
    }
  }
  guard let (loginwindowPid, field) = focusedField else {
    return .FocusUnavailable
  }
  // Match the supervised probe's explicit Command hold and release. A flagged
  // A pair alone did not exercise that proven lock-screen delivery sequence.
  guard ProcessInfo.processInfo.systemUptime < deadline else {
    return .DeadlineExceeded
  }
  command.0.flags = .maskCommand
  command.0.post(tap: .cghidEventTap)
  post(selectAll)
  command.1.post(tap: .cghidEventTap)
  Thread.sleep(forTimeInterval: 0.08)
  guard ProcessInfo.processInfo.systemUptime < deadline else {
    return .DeadlineExceeded
  }
  post(clear)
  Thread.sleep(forTimeInterval: 0.2)
  // NOTICE: loginwindow does not expose an AX character count for this field.
  // The supervised host probe verified this clear sequence visually; require
  // that gate again if the lock UI or input route changes.
  guard sameLockedField(uid: expected_uid, selector: selected, pid: loginwindowPid, field: field) else {
    return .FocusLost
  }

  for pair in characterEvents {
    guard ProcessInfo.processInfo.systemUptime < deadline else {
      return .DeadlineExceeded
    }
    guard sameLockedField(uid: expected_uid, selector: selected, pid: loginwindowPid, field: field) else {
      return .FocusLost
    }
    guard ProcessInfo.processInfo.systemUptime < deadline else {
      return .DeadlineExceeded
    }
    post(pair)
    Thread.sleep(forTimeInterval: 0.02)
  }
  Thread.sleep(forTimeInterval: 0.1)
  guard sameLockedField(uid: expected_uid, selector: selected, pid: loginwindowPid, field: field) else {
    return .FocusLost
  }
  guard ProcessInfo.processInfo.systemUptime < deadline else {
    return .DeadlineExceeded
  }
  post(submit)
  return .Submitted
}
