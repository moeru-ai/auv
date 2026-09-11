import AppKit

// Standalone native drawing regression probe; compile with CursorShadow.swift.
// This avoids linking unrelated Rust bridge symbols while testing the real renderer.
let shadow = CursorShadow(color: NSColor(srgbRed: 1, green: 0.6, blue: 0.15, alpha: 0.7), blurRadius: 8, offset: NSSize(width: 0, height: 2))
precondition(shadow.inset == 26, "Blur drawing space includes the native offset")
precondition(CursorShadow(color: .clear, blurRadius: 8, offset: .zero).inset == 0)

func render(shadow: CursorShadow?) -> NSBitmapImageRep {
  let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 96, pixelsHigh: 96, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
  NSGraphicsContext.saveGraphicsState()
  NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
  let draw = {
    let source = "<svg xmlns='http://www.w3.org/2000/svg' width='24' height='24' viewBox='0 0 24 24'><path d='M3 3Q2 2 2 4V21Q2 23 4 21L10 14H17Q20 14 17 12Z' fill='#c9c9c9' stroke='#eeeeee' stroke-width='1.5' stroke-linejoin='round'/></svg>"
    let image = NSImage(data: source.data(using: .utf8)!)!
    image.draw(in: NSRect(x: 36, y: 36, width: 24, height: 24))
  }
  if let shadow { shadow.draw(draw) } else { draw() }
  // A separate mark must not inherit the silhouette's graphics-state shadow.
  NSColor.white.setFill()
  NSRect(x: 80, y: 80, width: 4, height: 4).fill()
  NSGraphicsContext.restoreGraphicsState()
  return bitmap
}
let plain = render(shadow: nil)
let native = render(shadow: shadow)
var haloPixels = 0
for y in 0..<96 {
  for x in 0..<96 {
    let before = plain.colorAt(x: x, y: y)!.usingColorSpace(.deviceRGB)!
    let after = native.colorAt(x: x, y: y)!.usingColorSpace(.deviceRGB)!
    if before.alphaComponent < 0.001 && after.alphaComponent > 0.01 {
      haloPixels += 1
      precondition(after.redComponent > after.blueComponent, "Halo uses the supplied orange color")
      precondition(x > 0 && x < 95 && y > 0 && y < 95, "No visible clipping at bitmap boundary")
    }
    if x > 72 && y > 72 {
      precondition(abs(before.alphaComponent - after.alphaComponent) < 0.005, "Later drawing does not inherit the shadow")
    }
  }
}
precondition(haloPixels > 50, "Native blur creates a soft alpha silhouette outside the source")
print("Native shadow probe passed: \(haloPixels) halo pixels; graphics state restored")
