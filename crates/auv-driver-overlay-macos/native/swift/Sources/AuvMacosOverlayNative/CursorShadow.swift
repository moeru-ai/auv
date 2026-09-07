import AppKit

/// Native silhouette blur and drawing space, independent of the SVG canvas size.
/// The host controls the color; no product palette is embedded in this renderer.
struct CursorShadow {
  let color: NSColor
  let blurRadius: CGFloat
  let offset: NSSize

  /// Reserve three blur radii around the silhouette so the soft edge does not
  /// meet the transparent window boundary. Offset space is symmetric to keep
  /// view-to-screen positioning independent of the shadow direction.
  var inset: CGFloat {
    guard color.alphaComponent > 0 else { return 0 }
    return ceil(blurRadius * 3 + max(abs(offset.width), abs(offset.height)))
  }

  /// Composites all sprite paths once before blurring their combined alpha mask.
  /// Label pills and click ripples are drawn outside this graphics-state scope.
  /// See https://developer.apple.com/documentation/appkit/nsshadow.
  func draw(_ content: () -> Void) {
    guard let context = NSGraphicsContext.current?.cgContext else { return }
    NSGraphicsContext.saveGraphicsState()
    defer { NSGraphicsContext.restoreGraphicsState() }
    let shadow = NSShadow()
    shadow.shadowColor = color
    shadow.shadowBlurRadius = blurRadius
    shadow.shadowOffset = offset
    shadow.set()
    context.beginTransparencyLayer(auxiliaryInfo: nil)
    content()
    context.endTransparencyLayer()
  }
}
