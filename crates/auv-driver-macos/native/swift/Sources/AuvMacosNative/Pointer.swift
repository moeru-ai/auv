import AppKit
import CoreGraphics
import Darwin
import Foundation

private typealias CGEventSetWindowLocationFn = @convention(c) (
  CGEvent,
  CGPoint
) -> Void

private typealias SLEventPostToPidFn = @convention(c) (
  pid_t,
  CGEvent
) -> Void

private let cgEventSetWindowLocation: CGEventSetWindowLocationFn? = {
  let symbolName = "CGEventSetWindowLocation"
  let globalHandle = UnsafeMutableRawPointer(bitPattern: -2)
  if let symbol = dlsym(globalHandle, symbolName) {
    return unsafeBitCast(symbol, to: CGEventSetWindowLocationFn.self)
  }

  let skyLightPath = "/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight"
  guard let handle = dlopen(skyLightPath, RTLD_LAZY) else {
    return nil
  }
  guard let symbol = dlsym(handle, symbolName) else {
    return nil
  }
  return unsafeBitCast(symbol, to: CGEventSetWindowLocationFn.self)
}()

private let slEventPostToPid: SLEventPostToPidFn? = {
  let symbolName = "SLEventPostToPid"
  let skyLightPath = "/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight"
  _ = dlopen(skyLightPath, RTLD_LAZY | RTLD_GLOBAL)
  let globalHandle = UnsafeMutableRawPointer(bitPattern: -2)
  guard let symbol = dlsym(globalHandle, symbolName) else {
    return nil
  }
  return unsafeBitCast(symbol, to: SLEventPostToPidFn.self)
}()

private enum WindowClickStrategyCode: Int32 {
  case chromiumCompatible = 0
  case pidTargeted = 1
}

private final class TeachClickCaptureState {
  var point: CGPoint?
  var buttonCode: Int32 = 0
  var capturedAtUnixMs: Int64 = 0
}

private final class TeachClickReadyController: NSObject {
  weak var panel: NSPanel?

  @objc func ready(_ sender: Any?) {
    panel?.close()
    NSApp.stopModal()
  }
}

private func mouseButton(_ value: Int32) -> CGMouseButton {
  switch value {
  case 1: return .right
  case 2: return .center
  default: return .left
  }
}

private func mouseDownType(_ button: CGMouseButton) -> CGEventType {
  switch button {
  case .right: return .rightMouseDown
  case .center: return .otherMouseDown
  default: return .leftMouseDown
  }
}

private func mouseUpType(_ button: CGMouseButton) -> CGEventType {
  switch button {
  case .right: return .rightMouseUp
  case .center: return .otherMouseUp
  default: return .leftMouseUp
  }
}

private func buttonCode(for eventType: CGEventType) -> Int32 {
  switch eventType {
  case .rightMouseDown: return 1
  case .otherMouseDown: return 2
  default: return 0
  }
}

private func showTeachClickReadyPanel(prompt: String) {
  if !Thread.isMainThread {
    DispatchQueue.main.sync {
      showTeachClickReadyPanel(prompt: prompt)
    }
    return
  }

  let panel = NSPanel(
    contentRect: NSRect(x: 0, y: 0, width: 360, height: 140),
    styleMask: [.titled, .closable, .nonactivatingPanel],
    backing: .buffered,
    defer: false
  )
  panel.title = "AUV Teach Click"
  panel.level = .floating
  panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]

  let label = NSTextField(labelWithString: prompt)
  label.frame = NSRect(x: 20, y: 74, width: 320, height: 40)
  label.lineBreakMode = .byWordWrapping
  label.maximumNumberOfLines = 2

  let button = NSButton(title: "Ready", target: nil, action: nil)
  button.frame = NSRect(x: 248, y: 20, width: 92, height: 32)
  button.bezelStyle = .rounded
  let controller = TeachClickReadyController()
  controller.panel = panel
  button.target = controller
  button.action = #selector(TeachClickReadyController.ready(_:))

  panel.contentView?.addSubview(label)
  panel.contentView?.addSubview(button)
  objc_setAssociatedObject(
    panel,
    Unmanaged.passUnretained(panel).toOpaque(),
    controller,
    .OBJC_ASSOCIATION_RETAIN_NONATOMIC
  )
  panel.center()
  NSApp.activate(ignoringOtherApps: true)
  panel.makeKeyAndOrderFront(nil)
  NSApp.runModal(for: panel)
}

private func currentUnixMs() -> Int64 {
  Int64(Date().timeIntervalSince1970 * 1000.0)
}

private func waitForTaughtClick(timeoutMs: UInt64) -> NativeTeachClickResponse {
  let state = TeachClickCaptureState()
  let statePointer = Unmanaged.passRetained(state).toOpaque()
  let mask =
    (1 << CGEventType.leftMouseDown.rawValue)
    | (1 << CGEventType.rightMouseDown.rawValue)
    | (1 << CGEventType.otherMouseDown.rawValue)

  guard
    let tap = CGEvent.tapCreate(
      tap: .cgSessionEventTap,
      place: .headInsertEventTap,
      options: .listenOnly,
      eventsOfInterest: CGEventMask(mask),
      callback: { _, eventType, event, userInfo in
        guard let userInfo else {
          return Unmanaged.passUnretained(event)
        }
        let state = Unmanaged<TeachClickCaptureState>
          .fromOpaque(userInfo)
          .takeUnretainedValue()
        if state.point == nil {
          state.point = event.location
          state.buttonCode = buttonCode(for: eventType)
          state.capturedAtUnixMs = currentUnixMs()
        }
        CFRunLoopStop(CFRunLoopGetCurrent())
        return Unmanaged.passUnretained(event)
      },
      userInfo: statePointer
    )
  else {
    Unmanaged<TeachClickCaptureState>.fromOpaque(statePointer).release()
    return NativeTeachClickResponse(
      x: 0,
      y: 0,
      button_code: 0,
      captured_at_unix_ms: 0,
      error_message: "failed to create teach-click event tap".intoRustString(),
      recovery_hint: "grant Accessibility permission and retry".intoRustString()
    )
  }

  let runLoopSource = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, tap, 0)
  CFRunLoopAddSource(CFRunLoopGetCurrent(), runLoopSource, .commonModes)
  CGEvent.tapEnable(tap: tap, enable: true)

  let deadline = Date().addingTimeInterval(Double(timeoutMs) / 1000.0)
  while state.point == nil && Date() < deadline {
    CFRunLoopRunInMode(.defaultMode, 0.05, true)
  }

  CGEvent.tapEnable(tap: tap, enable: false)
  CFRunLoopRemoveSource(CFRunLoopGetCurrent(), runLoopSource, .commonModes)
  Unmanaged<TeachClickCaptureState>.fromOpaque(statePointer).release()

  guard let point = state.point else {
    return NativeTeachClickResponse(
      x: 0,
      y: 0,
      button_code: 0,
      captured_at_unix_ms: 0,
      error_message: "timed out waiting for taught click".intoRustString(),
      recovery_hint: "run debug.teachClick again and click the target before timeout_ms expires".intoRustString()
    )
  }

  return NativeTeachClickResponse(
    x: point.x,
    y: point.y,
    button_code: state.buttonCode,
    captured_at_unix_ms: state.capturedAtUnixMs,
    error_message: nil,
    recovery_hint: nil
  )
}

private func setRawIntegerField(_ event: CGEvent, _ raw: UInt32, _ value: Int64) {
  if let field = CGEventField(rawValue: raw) {
    event.setIntegerValueField(field, value: value)
  }
}

private func stampCompatibilityMouseEvent(
  _ event: CGEvent,
  pid: Int64,
  windowNumber: Int64,
  windowLocation: CGPoint,
  clickGroupId: Int64,
  clickState: Int64,
  phase: Int64,
  buttonNumber: Int64,
  setWindowLocation: CGEventSetWindowLocationFn
) {
  setRawIntegerField(event, 0, phase)
  setRawIntegerField(event, 1, clickState)
  setRawIntegerField(event, 3, buttonNumber)
  setRawIntegerField(event, 7, 3)
  setRawIntegerField(event, 40, pid)
  setRawIntegerField(event, 51, windowNumber)
  setRawIntegerField(event, 58, clickGroupId)
  setRawIntegerField(event, 91, windowNumber)
  setRawIntegerField(event, 92, windowNumber)
  setWindowLocation(event, windowLocation)
}

private func postCompatibilityMouseEvent(_ event: CGEvent, pid: Int64) {
  slEventPostToPid?(pid_t(pid), event)
  event.postToPid(pid_t(pid))
}

private func stampPidTargetedMouseEvent(
  _ event: CGEvent,
  pid: Int64,
  windowNumber: Int64,
  windowLocation: CGPoint,
  clickState: Int64,
  buttonNumber: Int64,
  setWindowLocation: CGEventSetWindowLocationFn
) {
  event.setIntegerValueField(.eventTargetUnixProcessID, value: pid)
  event.setIntegerValueField(.mouseEventClickState, value: clickState)
  event.setIntegerValueField(.mouseEventWindowUnderMousePointer, value: windowNumber)
  event.setIntegerValueField(.mouseEventWindowUnderMousePointerThatCanHandleThisEvent, value: windowNumber)
  setRawIntegerField(event, 3, buttonNumber)
  setRawIntegerField(event, 40, pid)
  setRawIntegerField(event, 51, windowNumber)
  setRawIntegerField(event, 91, windowNumber)
  setRawIntegerField(event, 92, windowNumber)
  setWindowLocation(event, windowLocation)
}

private func runPidTargetedWindowClick(
  source: CGEventSource?,
  pid: Int64,
  windowNumber: Int64,
  button: CGMouseButton,
  screenLocation: CGPoint,
  windowLocation: CGPoint,
  clickCount: Int64,
  clickIntervalMicros: useconds_t,
  setWindowLocation: CGEventSetWindowLocationFn
) -> NativeActionResponse {
  let buttonNumber: Int64 = switch button {
  case .right: 1
  case .center: 2
  default: 0
  }

  for clickNumber in 1...clickCount {
    guard
      let down = CGEvent(
        mouseEventSource: source,
        mouseType: mouseDownType(button),
        mouseCursorPosition: screenLocation,
        mouseButton: button
      ),
      let up = CGEvent(
        mouseEventSource: source,
        mouseType: mouseUpType(button),
        mouseCursorPosition: screenLocation,
        mouseButton: button
      )
    else {
      return nativeActionError(
        "failed to create window-targeted mouse click event",
        "grant Accessibility permission and retry"
      )
    }
    stampPidTargetedMouseEvent(
      down,
      pid: pid,
      windowNumber: windowNumber,
      windowLocation: windowLocation,
      clickState: clickNumber,
      buttonNumber: buttonNumber,
      setWindowLocation: setWindowLocation
    )
    stampPidTargetedMouseEvent(
      up,
      pid: pid,
      windowNumber: windowNumber,
      windowLocation: windowLocation,
      clickState: clickNumber,
      buttonNumber: buttonNumber,
      setWindowLocation: setWindowLocation
    )
    down.postToPid(pid_t(pid))
    up.postToPid(pid_t(pid))
    if clickNumber < clickCount {
      usleep(clickIntervalMicros)
    }
  }

  return nativeActionOk()
}

private func runCompatibilityWindowClick(
  source: CGEventSource?,
  pid: Int64,
  windowNumber: Int64,
  button: CGMouseButton,
  screenLocation: CGPoint,
  windowLocation: CGPoint,
  clickCount: Int,
  clickIntervalMicros: useconds_t,
  setWindowLocation: CGEventSetWindowLocationFn
) -> NativeActionResponse {
  let clickGroupId = Int64(Date().timeIntervalSince1970 * 1_000_000) & 0x7fff_ffff
  let offscreen = CGPoint(x: -1, y: -1)
  let offscreenLocal = CGPoint(x: -1, y: -1)
  let clickPairs = min(max(clickCount, 1), 2)
  let buttonNumber: Int64 = switch button {
  case .right: 1
  case .center: 2
  default: 0
  }

  guard
    let move = CGEvent(
      mouseEventSource: source,
      mouseType: .mouseMoved,
      mouseCursorPosition: screenLocation,
      mouseButton: button
    )
  else {
    return nativeActionError(
      "failed to create compatibility mouse move event",
      "grant Accessibility permission and retry"
    )
  }
  stampCompatibilityMouseEvent(
    move,
    pid: pid,
    windowNumber: windowNumber,
    windowLocation: windowLocation,
    clickGroupId: clickGroupId,
    clickState: 0,
    phase: 2,
    buttonNumber: buttonNumber,
    setWindowLocation: setWindowLocation
  )
  postCompatibilityMouseEvent(move, pid: pid)
  usleep(15_000)

  guard
    let primerDown = CGEvent(
      mouseEventSource: source,
      mouseType: mouseDownType(button),
      mouseCursorPosition: offscreen,
      mouseButton: button
    ),
    let primerUp = CGEvent(
      mouseEventSource: source,
      mouseType: mouseUpType(button),
      mouseCursorPosition: offscreen,
      mouseButton: button
    )
  else {
    return nativeActionError(
      "failed to create compatibility primer click events",
      "grant Accessibility permission and retry"
    )
  }
  stampCompatibilityMouseEvent(
    primerDown,
    pid: pid,
    windowNumber: windowNumber,
    windowLocation: offscreenLocal,
    clickGroupId: clickGroupId,
    clickState: 1,
    phase: 1,
    buttonNumber: buttonNumber,
    setWindowLocation: setWindowLocation
  )
  stampCompatibilityMouseEvent(
    primerUp,
    pid: pid,
    windowNumber: windowNumber,
    windowLocation: offscreenLocal,
    clickGroupId: clickGroupId,
    clickState: 1,
    phase: 2,
    buttonNumber: buttonNumber,
    setWindowLocation: setWindowLocation
  )
  postCompatibilityMouseEvent(primerDown, pid: pid)
  usleep(1_000)
  postCompatibilityMouseEvent(primerUp, pid: pid)
  usleep(100_000)

  for pairIndex in 1...clickPairs {
    guard
      let down = CGEvent(
        mouseEventSource: source,
        mouseType: mouseDownType(button),
        mouseCursorPosition: screenLocation,
        mouseButton: button
      ),
      let up = CGEvent(
        mouseEventSource: source,
        mouseType: mouseUpType(button),
        mouseCursorPosition: screenLocation,
        mouseButton: button
      )
    else {
      return nativeActionError(
        "failed to create compatibility target click events",
        "grant Accessibility permission and retry"
      )
    }
    let clickState = Int64(pairIndex)
    stampCompatibilityMouseEvent(
      down,
      pid: pid,
      windowNumber: windowNumber,
      windowLocation: windowLocation,
      clickGroupId: clickGroupId,
      clickState: clickState,
      phase: 3,
      buttonNumber: buttonNumber,
      setWindowLocation: setWindowLocation
    )
    stampCompatibilityMouseEvent(
      up,
      pid: pid,
      windowNumber: windowNumber,
      windowLocation: windowLocation,
      clickGroupId: clickGroupId,
      clickState: clickState,
      phase: 3,
      buttonNumber: buttonNumber,
      setWindowLocation: setWindowLocation
    )
    postCompatibilityMouseEvent(down, pid: pid)
    usleep(1_000)
    postCompatibilityMouseEvent(up, pid: pid)
    if pairIndex < clickPairs {
      usleep(clickIntervalMicros)
    }
  }

  return nativeActionOk()
}

func click_point(
  x: Double,
  y: Double,
  button_code: Int32,
  click_count: Int64,
  click_interval_ms: UInt64
) -> NativeActionResponse {
  let button = mouseButton(button_code)
  let clickCount = max(click_count, 1)
  let clickIntervalSeconds = Double(click_interval_ms) / 1000.0
  let location = CGPoint(x: x, y: y)
  let originalLocation = CGEvent(source: nil)?.location ?? location

  CGWarpMouseCursorPosition(location)
  defer {
    CGWarpMouseCursorPosition(originalLocation)
  }

  // NOTICE: Some apps or canvas-backed controls ignore a down/up pair posted
  // immediately after a cursor warp because they have not observed pointer
  // motion into the hit target yet. If a click reports success but the UI does
  // not react, posting a real move event and giving the event tap a short
  // settle is a useful first mitigation to try.
  if let move = CGEvent(
    mouseEventSource: nil,
    mouseType: .mouseMoved,
    mouseCursorPosition: location,
    mouseButton: button
  ) {
    move.post(tap: .cghidEventTap)
    usleep(15_000)
  }

  for clickNumber in 1...clickCount {
    guard
      let down = CGEvent(
        mouseEventSource: nil,
        mouseType: mouseDownType(button),
        mouseCursorPosition: location,
        mouseButton: button
      ),
      let up = CGEvent(
        mouseEventSource: nil,
        mouseType: mouseUpType(button),
        mouseCursorPosition: location,
        mouseButton: button
      )
    else {
      return nativeActionError(
        "failed to create mouse click event",
        "grant Accessibility permission and retry"
      )
    }
    down.setIntegerValueField(.mouseEventClickState, value: clickNumber)
    up.setIntegerValueField(.mouseEventClickState, value: clickNumber)
    down.post(tap: .cghidEventTap)
    up.post(tap: .cghidEventTap)
    if clickNumber < clickCount && clickIntervalSeconds > 0 {
      Thread.sleep(forTimeInterval: clickIntervalSeconds)
    }
  }

  return nativeActionOk()
}

func click_window_point(
  pid: Int64,
  window_number: Int64,
  screen_x: Double,
  screen_y: Double,
  window_x: Double,
  window_y: Double,
  button_code: Int32,
  click_count: Int64,
  click_interval_ms: UInt64,
  window_strategy_code: Int32
) -> NativeActionResponse {
  let button = mouseButton(button_code)
  let clickCount = max(click_count, 1)
  guard let strategy = WindowClickStrategyCode(rawValue: window_strategy_code) else {
    return nativeActionError(
      "unsupported window click strategy",
      "use a WindowClickStrategy value supported by this macOS driver build"
    )
  }
  let clickIntervalMicros = useconds_t(min(click_interval_ms, UInt64(useconds_t.max) / 1000) * 1000)
  let screenLocation = CGPoint(x: screen_x, y: screen_y)
  // NOTICE: `CGEventSetWindowLocation` is used here as a routing hint for
  // background window delivery. AUV's `WindowPoint` and CUA's macOS pixel path
  // both use screenshot/window-local coordinates with a top-left origin, so keep
  // that y value intact when stamping the event. Flipping to `windowHeight - y`
  // makes canvas-style apps such as NetEase Cloud Music consume the click near
  // the bottom of the window even when the event's screen point is visually on
  // the search box.
  //
  // References:
  // https://github.com/trycua/cua/blob/a3448588286b6373013a5fa9072ac8bafb6681d6/libs/cua-driver-rs/crates/platform-macos/src/input/mouse.rs#L388-L413
  let windowLocation = CGPoint(x: window_x, y: window_y)
  guard let setWindowLocation = cgEventSetWindowLocation else {
    return nativeActionError(
      "unsupported window-local event location",
      "CGEventSetWindowLocation is unavailable; retry on a macOS version that exposes the SkyLight symbol"
    )
  }

  if strategy == .chromiumCompatible {
    // NOTICE: Chromium/WebView/Catalyst targets can ignore plain
    // `CGEvent.postToPid` mouse pairs even when AppKit receives them. Keep this
    // strategy explicit at the Rust API layer because it is more invasive than a
    // direct pid-targeted click: it posts a move, an off-screen primer click,
    // then the target click pair(s).
    //
    // WORKAROUND: NetEase Cloud Music accepts this CUA-style Chromium sequence
    // but did not react to plain `postToPid`, `cghidEventTap`, or pid+HID
    // variants during `2026-05-27` live testing. The primer and raw event fields
    // keep Chromium's user-activation and synthetic-event filters satisfied
    // without foregrounding the target app.
    //
    // References:
    // https://github.com/trycua/cua/blob/a3448588286b6373013a5fa9072ac8bafb6681d6/libs/cua-driver-rs/crates/platform-macos/src/input/mouse.rs#L103-L224
    // https://github.com/trycua/cua/blob/a3448588286b6373013a5fa9072ac8bafb6681d6/libs/cua-driver-rs/crates/platform-macos/src/input/mouse.rs#L383-L438
    return runCompatibilityWindowClick(
      source: nil,
      pid: pid,
      windowNumber: window_number,
      button: button,
      screenLocation: screenLocation,
      windowLocation: windowLocation,
      clickCount: Int(clickCount),
      clickIntervalMicros: clickIntervalMicros,
      setWindowLocation: setWindowLocation
    )
  }

  // NOTICE: This is the narrower public pid-targeted route. It is useful for
  // AppKit targets and for comparing delivery behavior, but Chromium/WebView
  // apps may ignore it while still reporting native post success.
  //
  // References:
  // https://github.com/EYHN/kwwk-computer-use-core/blob/eddd9e5475095de58bcb81cafbad79d1f5c5495d/Sources/KWWKComputerUseCore/BackgroundInputDispatcher.swift#L130-L162
  return runPidTargetedWindowClick(
    source: nil,
    pid: pid,
    windowNumber: window_number,
    button: button,
    screenLocation: screenLocation,
    windowLocation: windowLocation,
    clickCount: clickCount,
    clickIntervalMicros: clickIntervalMicros,
    setWindowLocation: setWindowLocation
  )
}

func current_mouse_location() -> NativeMouseLocationResponse {
  let location = NSEvent.mouseLocation
  let referenceHeight =
    Double(NSScreen.main?.frame.height ?? NSScreen.screens.first?.frame.height ?? 0)
  return NativeMouseLocationResponse(
    x: location.x,
    y: referenceHeight - location.y,
    error_message: nil,
    recovery_hint: nil
  )
}

func teach_next_click(prompt: RustString, timeout_ms: UInt64) -> NativeTeachClickResponse {
  let promptText = nativeSanitize(prompt.toString())
  showTeachClickReadyPanel(prompt: promptText)
  return waitForTaughtClick(timeoutMs: max(timeout_ms, 1))
}

func scroll_point(x: Double, y: Double, delta_x: Double, delta_y: Double) -> NativeActionResponse {
  let location = CGPoint(x: x, y: y)
  let originalLocation = CGEvent(source: nil)?.location ?? location

  CGWarpMouseCursorPosition(location)
  defer {
    CGWarpMouseCursorPosition(originalLocation)
  }

  if let moveEvent = CGEvent(
    mouseEventSource: nil,
    mouseType: .mouseMoved,
    mouseCursorPosition: location,
    mouseButton: .left
  ) {
    moveEvent.post(tap: .cghidEventTap)
  }

  guard
    let scrollEvent = CGEvent(
      scrollWheelEvent2Source: nil,
      units: .pixel,
      wheelCount: 2,
      wheel1: Int32(delta_y.rounded()),
      wheel2: Int32(delta_x.rounded()),
      wheel3: 0
    )
  else {
    return nativeActionError(
      "failed to create scroll event",
      "grant Accessibility permission and retry"
    )
  }
  scrollEvent.post(tap: .cghidEventTap)

  return nativeActionOk()
}

func scroll_window_point(
  pid: Int64,
  window_number: Int64,
  screen_x: Double,
  screen_y: Double,
  window_x: Double,
  window_y: Double,
  delta_x: Double,
  delta_y: Double
) -> NativeActionResponse {
  let screenLocation = CGPoint(x: screen_x, y: screen_y)
  let windowLocation = CGPoint(x: window_x, y: window_y)
  guard let setWindowLocation = cgEventSetWindowLocation else {
    return nativeActionError(
      "CGEventSetWindowLocation is unavailable",
      "retry on a macOS version that exposes the SkyLight symbol"
    )
  }
  guard
    let scrollEvent = CGEvent(
      scrollWheelEvent2Source: nil,
      units: .pixel,
      wheelCount: 2,
      wheel1: Int32(delta_y.rounded()),
      wheel2: Int32(delta_x.rounded()),
      wheel3: 0
    )
  else {
    return nativeActionError(
      "failed to create window-targeted scroll event",
      "grant Accessibility permission and retry"
    )
  }

  scrollEvent.location = screenLocation
  scrollEvent.setIntegerValueField(.eventTargetUnixProcessID, value: pid)
  setRawIntegerField(scrollEvent, 40, pid)
  setRawIntegerField(scrollEvent, 51, window_number)
  setRawIntegerField(scrollEvent, 91, window_number)
  setRawIntegerField(scrollEvent, 92, window_number)
  setWindowLocation(scrollEvent, windowLocation)

  if let slEventPostToPid {
    slEventPostToPid(pid_t(pid), scrollEvent)
  } else {
    scrollEvent.postToPid(pid_t(pid))
  }

  return nativeActionOk()
}
