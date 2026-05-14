import CoreGraphics
import Foundation

let location = CGPoint(x: __X__, y: __Y__)
let deltaX = Int32(__DELTA_X__)
let deltaY = Int32(__DELTA_Y__)
let originalLocation = CGEvent(source: nil)?.location ?? location

defer {
  CGWarpMouseCursorPosition(originalLocation)
}

CGWarpMouseCursorPosition(location)

if let moveEvent = CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: location, mouseButton: .left) {
  moveEvent.post(tap: .cghidEventTap)
}

if let scrollEvent = CGEvent(scrollWheelEvent2Source: nil, units: .pixel, wheelCount: 2, wheel1: deltaY, wheel2: deltaX, wheel3: 0) {
  scrollEvent.post(tap: .cghidEventTap)
}

print("status=scrolled")
