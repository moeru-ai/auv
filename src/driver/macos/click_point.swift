import CoreGraphics
import Foundation

func mouseButton(_ value: Int) -> CGMouseButton {
  switch value {
  case 1: return .right
  case 2: return .center
  default: return .left
  }
}

func mouseDownType(_ button: CGMouseButton) -> CGEventType {
  switch button {
  case .right: return .rightMouseDown
  case .center: return .otherMouseDown
  default: return .leftMouseDown
  }
}

func mouseUpType(_ button: CGMouseButton) -> CGEventType {
  switch button {
  case .right: return .rightMouseUp
  case .center: return .otherMouseUp
  default: return .leftMouseUp
  }
}

let button = mouseButton(__BUTTON__)
let clickCount = max(__CLICK_COUNT__, 1)
let location = CGPoint(x: __X__, y: __Y__)
let originalLocation = CGEvent(source: nil)?.location ?? location

defer {
  CGWarpMouseCursorPosition(originalLocation)
}

CGWarpMouseCursorPosition(location)

for _ in 0..<clickCount {
  if let down = CGEvent(mouseEventSource: nil, mouseType: mouseDownType(button), mouseCursorPosition: location, mouseButton: button),
     let up = CGEvent(mouseEventSource: nil, mouseType: mouseUpType(button), mouseCursorPosition: location, mouseButton: button) {
    down.setIntegerValueField(.mouseEventClickState, value: Int64(clickCount))
    up.setIntegerValueField(.mouseEventClickState, value: Int64(clickCount))
    down.post(tap: .cghidEventTap)
    up.post(tap: .cghidEventTap)
  }
}

print("status=clicked")
