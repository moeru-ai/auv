import AppKit
import CoreGraphics
import Foundation

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
