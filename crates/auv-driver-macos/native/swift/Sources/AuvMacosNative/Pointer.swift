import AppKit
import CoreGraphics
import Darwin
import Foundation

private typealias CGEventSetWindowLocationFn = @convention(c) (
  CGEvent,
  CGPoint
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

private func stampWindowTarget(
  _ event: CGEvent,
  pid: Int64,
  windowNumber: Int64,
  windowLocation: CGPoint
) {
  event.setIntegerValueField(.eventTargetUnixProcessID, value: pid)
  event.setIntegerValueField(.mouseEventWindowUnderMousePointer, value: windowNumber)
  event.setIntegerValueField(.mouseEventWindowUnderMousePointerThatCanHandleThisEvent, value: windowNumber)
  if let eventWindowNumber = CGEventField(rawValue: 51) {
    event.setIntegerValueField(eventWindowNumber, value: windowNumber)
  }
  if let eventWindowId = CGEventField(rawValue: 40) {
    event.setIntegerValueField(eventWindowId, value: windowNumber)
  }
  cgEventSetWindowLocation?(event, windowLocation)
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
  click_interval_ms: UInt64
) -> NativeActionResponse {
  let button = mouseButton(button_code)
  let clickCount = max(click_count, 1)
  let clickIntervalSeconds = Double(click_interval_ms) / 1000.0
  let screenLocation = CGPoint(x: screen_x, y: screen_y)
  let windowLocation = CGPoint(x: window_x, y: window_y)

  for clickNumber in 1...clickCount {
    guard
      let down = CGEvent(
        mouseEventSource: nil,
        mouseType: mouseDownType(button),
        mouseCursorPosition: screenLocation,
        mouseButton: button
      ),
      let up = CGEvent(
        mouseEventSource: nil,
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

    down.setIntegerValueField(.mouseEventClickState, value: clickNumber)
    up.setIntegerValueField(.mouseEventClickState, value: clickNumber)

    // Provenance: CUA mouse and KWWK mouse background dispatch patterns.
    // https://github.com/trycua/cua/blob/a3448588286b6373013a5fa9072ac8bafb6681d6/libs/cua-driver-rs/crates/platform-macos/src/input/mouse.rs#L383-L438
    // https://github.com/EYHN/kwwk-computer-use-core/blob/eddd9e5475095de58bcb81cafbad79d1f5c5495d/Sources/KWWKComputerUseCore/BackgroundInputDispatcher.swift#L130-L162
    stampWindowTarget(down, pid: pid, windowNumber: window_number, windowLocation: windowLocation)
    stampWindowTarget(up, pid: pid, windowNumber: window_number, windowLocation: windowLocation)

    down.postToPid(pid_t(pid))
    up.postToPid(pid_t(pid))
    if clickNumber < clickCount && clickIntervalSeconds > 0 {
      Thread.sleep(forTimeInterval: clickIntervalSeconds)
    }
  }

  return nativeActionOk()
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
