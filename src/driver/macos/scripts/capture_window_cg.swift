import Cocoa
import CoreGraphics
import ImageIO

let windowNumber = CGWindowID(__WINDOW_NUMBER__)
let outputPath: String = __OUTPUT_PATH__

// Enumerate all on-screen windows WITHOUT the kCGWindowSharingState filter that xcap applies.
// xcap skips windows with kCGWindowSharingState==0 ("not shared"), but
// CGWindowListCreateImage can still capture them if Screen Recording is granted.
let listOptions = CGWindowListOption.optionOnScreenOnly.union(.excludeDesktopElements)
guard let windowList = CGWindowListCopyWindowInfo(listOptions, kCGNullWindowID)
        as? [[String: Any]] else {
  fputs("error: CGWindowListCopyWindowInfo failed\n", stderr)
  exit(1)
}

var windowBounds: CGRect? = nil
var ownerPid: Int32 = 0
var ownerName = ""

for entry in windowList {
  guard let num = entry[kCGWindowNumber as String] as? Int,
        CGWindowID(exactly: UInt64(num)) == windowNumber else { continue }
  ownerName = entry[kCGWindowOwnerName as String] as? String ?? ""
  ownerPid  = Int32(entry[kCGWindowOwnerPID as String] as? Int ?? 0)
  if let bd = entry[kCGWindowBounds as String] {
    var rect = CGRect.zero
    if CGRectMakeWithDictionaryRepresentation(bd as! CFDictionary, &rect) {
      windowBounds = rect
    }
  }
  break
}

guard let bounds = windowBounds else {
  fputs("error: window \(windowNumber) not found in CGWindowList\n", stderr)
  exit(1)
}

// CGWindowListCreateImage with the window's own bounds rect.
// OptionIncludingWindow renders just this window (not others behind it).
// Default image option includes the shadow framing.
let cgImage = CGWindowListCreateImage(
  bounds,
  .optionIncludingWindow,
  windowNumber,
  .default
)

guard let image = cgImage else {
  fputs("error: CGWindowListCreateImage returned nil for window \(windowNumber)\n", stderr)
  exit(2)
}

// Write PNG via ImageIO (faster than NSImage round-trip).
let url = URL(fileURLWithPath: outputPath)
guard let dest = CGImageDestinationCreateWithURL(
  url as CFURL, "public.png" as CFString, 1, nil
) else {
  fputs("error: CGImageDestinationCreateWithURL failed for \(outputPath)\n", stderr)
  exit(2)
}
CGImageDestinationAddImage(dest, image, nil)
guard CGImageDestinationFinalize(dest) else {
  fputs("error: CGImageDestinationFinalize failed for \(outputPath)\n", stderr)
  exit(2)
}

print("windowNumber=\(windowNumber)")
print("pixelWidth=\(image.width)")
print("pixelHeight=\(image.height)")
print("logicalX=\(bounds.origin.x)")
print("logicalY=\(bounds.origin.y)")
print("logicalWidth=\(bounds.size.width)")
print("logicalHeight=\(bounds.size.height)")
print("ownerPid=\(ownerPid)")
print("ownerName=\(ownerName)")
