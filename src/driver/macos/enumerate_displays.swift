import AppKit
import CoreGraphics
import Foundation

let screens = NSScreen.screens
let referenceHeight = NSScreen.main?.frame.height ?? screens.first?.frame.height ?? 0

print("capturedAt=\(ISO8601DateFormatter().string(from: Date()))")
print("displayCount=\(screens.count)")

for screen in screens {
  let deviceDescription = screen.deviceDescription
  let displayId = (deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?.uint32Value ?? 0
  let frame = screen.frame
  let visibleFrame = screen.visibleFrame
  let scaleFactor = screen.backingScaleFactor

  let boundsX = Int(frame.origin.x.rounded())
  let boundsY = Int((referenceHeight - frame.origin.y - frame.height).rounded())
  let boundsWidth = Int(frame.width.rounded())
  let boundsHeight = Int(frame.height.rounded())

  let visibleX = Int(visibleFrame.origin.x.rounded())
  let visibleY = Int((referenceHeight - visibleFrame.origin.y - visibleFrame.height).rounded())
  let visibleWidth = Int(visibleFrame.width.rounded())
  let visibleHeight = Int(visibleFrame.height.rounded())

  let isMain = CGDisplayIsMain(displayId) != 0
  let isBuiltIn = CGDisplayIsBuiltin(displayId) != 0
  let pixelWidth = Int((frame.width * scaleFactor).rounded())
  let pixelHeight = Int((frame.height * scaleFactor).rounded())
  let scaleText = String(format: "%.3f", scaleFactor)

  print(
    "display\t\(displayId)\t\(isMain ? 1 : 0)\t\(isBuiltIn ? 1 : 0)\t\(boundsX)\t\(boundsY)\t\(boundsWidth)\t\(boundsHeight)\t\(visibleX)\t\(visibleY)\t\(visibleWidth)\t\(visibleHeight)\t\(scaleText)\t\(pixelWidth)\t\(pixelHeight)"
  )
}
