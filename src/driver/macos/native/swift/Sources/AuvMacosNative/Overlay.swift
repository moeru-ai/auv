import AppKit
import Foundation

/// Pixel-art cursor sprite ported from the AUV design system.
///
/// Each entry is a 2x2 cell on a 24x24 sprite grid (matches the SVG
/// viewBox in `docs/design/assets/cursor-auv.svg` and friends). The
/// `width` / `height` are in cells (1 cell = 2 sprite-px); the
/// renderer scales the whole grid to the requested output size.
struct AuvPixelCell {
  let x: Int
  let y: Int
  let width: Int
  let height: Int
  let color: NSColor
}

/// Two-color brand pill matching the design system's cursor labels
/// (`auv` cyan-strong, `you` slate). Colors are sampled from the
/// Moeru AI org palette in `colors_and_type.css`.
enum AuvOverlayCursorVariant {
  /// AUV replay cursor — cyan body + lime crook accent. Default for
  /// every Phase 2/3 driver command that wraps a non-cursor-touching
  /// action with `with_overlay_cursor`.
  case auv
  /// AUV click cursor — replay cursor plus the lime burst from
  /// `docs/design/assets/cursor-auv-click.svg`.
  case auvClick
  /// User cursor decoration (illustration-only — macOS does not let
  /// us repaint the hardware cursor; this variant exists so an
  /// inspect-viewer mock or future trace overlay can render it on
  /// top of a screenshot).
  case you

  var sprite: [AuvPixelCell] {
    switch self {
    case .auv: return auvSprite()
    case .auvClick: return auvClickSprite()
    case .you: return youSprite()
    }
  }

  var pillBackground: NSColor {
    switch self {
    // --auv-brand-strong (#009ba6)
    case .auv, .auvClick: return NSColor(srgbRed: 0.0, green: 0.608, blue: 0.651, alpha: 1.0)
    // slate (#2a3a52)
    case .you: return NSColor(srgbRed: 0.164, green: 0.227, blue: 0.322, alpha: 1.0)
    }
  }
}

final class NativeOverlayCursorView: NSView {
  var label: String = "auv · replay"
  var variant: AuvOverlayCursorVariant = .auv

  override var isFlipped: Bool {
    true
  }

  override func draw(_ dirtyRect: NSRect) {
    super.draw(dirtyRect)

    // Layout matches preview/brand-replay-cursor.html: 24pt sprite +
    // 8pt gap + pill that auto-sizes to the label width. The view's
    // frame is laid out to fit; everything draws against (0, 0) in
    // flipped (top-left origin) coordinates.
    let spriteSize: CGFloat = 24
    let spriteOrigin = NSPoint(x: 0, y: 0)
    drawPixelSprite(variant.sprite, origin: spriteOrigin, outputSize: spriteSize)

    // Label pill — mono 11pt, white text, brand background, 999px
    // pill radius. Padding 3/8 per design.
    let pillFont = NSFont.monospacedSystemFont(ofSize: 11, weight: .semibold)
    let pillAttributes: [NSAttributedString.Key: Any] = [
      .font: pillFont,
      .foregroundColor: NSColor.white,
    ]
    let textSize = (label as NSString).size(withAttributes: pillAttributes)
    let pillPaddingX: CGFloat = 8
    let pillPaddingY: CGFloat = 3
    let pillWidth = ceil(textSize.width) + pillPaddingX * 2
    let pillHeight = ceil(textSize.height) + pillPaddingY * 2
    let pillOriginX = spriteSize + 6
    let pillOriginY = (spriteSize - pillHeight) / 2
    let pillRect = NSRect(x: pillOriginX, y: pillOriginY, width: pillWidth, height: pillHeight)
    let pillPath = NSBezierPath(roundedRect: pillRect, xRadius: pillHeight / 2, yRadius: pillHeight / 2)
    variant.pillBackground.setFill()
    pillPath.fill()

    let textRect = NSRect(
      x: pillRect.minX + pillPaddingX,
      y: pillRect.minY + pillPaddingY,
      width: pillRect.width - pillPaddingX * 2,
      height: pillRect.height - pillPaddingY * 2
    )
    (label as NSString).draw(in: textRect, withAttributes: pillAttributes)
  }

  /// Compute the smallest frame that fits a sprite + label pill.
  /// Used by the controller to resize the host window so the pill
  /// never gets clipped by a fixed-width frame.
  func intrinsicLayoutSize() -> NSSize {
    let spriteSize: CGFloat = 24
    let pillFont = NSFont.monospacedSystemFont(ofSize: 11, weight: .semibold)
    let textSize = (label as NSString).size(withAttributes: [.font: pillFont])
    let pillWidth = ceil(textSize.width) + 16
    let pillHeight = ceil(textSize.height) + 6
    let width = spriteSize + 6 + pillWidth
    let height = max(spriteSize, pillHeight)
    return NSSize(width: width, height: height)
  }

  private func drawPixelSprite(
    _ cells: [AuvPixelCell],
    origin: NSPoint,
    outputSize: CGFloat
  ) {
    // Native grid is 24 sprite-units wide (12 cells of 2 units). The
    // sprite uses isFlipped so cell (0,0) is top-left.
    let cellPt = outputSize / 24.0
    for cell in cells {
      cell.color.setFill()
      let rect = NSRect(
        x: origin.x + CGFloat(cell.x) * cellPt,
        y: origin.y + CGFloat(cell.y) * cellPt,
        width: CGFloat(cell.width) * cellPt,
        height: CGFloat(cell.height) * cellPt
      )
      rect.fill()
    }
  }
}

final class NativeOverlayCursorState {
  let window: NSWindow
  let view: NativeOverlayCursorView
  var logicalX: Double?
  var logicalY: Double?
  var label: String
  var variant: AuvOverlayCursorVariant

  init(window: NSWindow, view: NativeOverlayCursorView, label: String, variant: AuvOverlayCursorVariant) {
    self.window = window
    self.view = view
    self.label = label
    self.variant = variant
  }
}

final class NativeOverlayController {
  private var cursors: [String: NativeOverlayCursorState] = [:]
  private var userCursorTrackingTimer: Timer?
  private var userCursorTrackingLabel: String = "you"

  func show_overlay_cursor(x: Double, y: Double, label: RustString) -> NativeActionResponse {
    runOnMain {
      let resolvedLabel = label.toString()
      self.hideCursor(id: "you")
      self.placeCursor(
        state: self.ensureCursor(id: "auv", label: resolvedLabel, variant: .auv),
        x: x,
        y: y,
        label: resolvedLabel,
        variant: .auv
      )
    }
  }

  func show_overlay_dual_cursor(
    x: Double,
    y: Double,
    label: RustString,
    user_label: RustString
  ) -> NativeActionResponse {
    runOnMain {
      let resolvedLabel = label.toString()
      let resolvedUserLabel = user_label.toString()
      self.startUserCursorTracking(label: resolvedUserLabel)
      self.placeCursor(
        state: self.ensureCursor(id: "auv", label: resolvedLabel, variant: .auv),
        x: x,
        y: y,
        label: resolvedLabel.isEmpty ? "auv · replay" : resolvedLabel,
        variant: .auv
      )
    }
  }

  func move_overlay_dual_cursor(
    x: Double,
    y: Double,
    label: RustString,
    user_label: RustString,
    duration_ms: UInt64
  ) -> NativeActionResponse {
    runOnMain {
      let resolvedLabel = label.toString()
      let resolvedUserLabel = user_label.toString()
      let userPoint = self.currentMouseLogicalPoint()
      self.startUserCursorTracking(label: resolvedUserLabel)

      let duration = max(0.0, Double(duration_ms) / 1000.0)
      let targetLabel = resolvedLabel.isEmpty ? "auv · replay" : resolvedLabel
      self.moveCursor(
        state: self.ensureCursor(id: "auv", label: targetLabel, variant: .auv),
        targetX: x,
        targetY: y,
        label: targetLabel,
        variant: .auv,
        duration: duration,
        startPoint: userPoint
      )
    }
  }

  func set_overlay_cursor(
    cursor_id: RustString,
    x: Double,
    y: Double,
    label: RustString,
    variant: RustString
  ) -> NativeActionResponse {
    runOnMain {
      let resolvedId = self.normalizeCursorId(cursor_id.toString(), fallback: "auv")
      let resolvedLabel = label.toString()
      let resolvedVariant = self.variantFrom(variant.toString())
      self.placeCursor(
        state: self.ensureCursor(id: resolvedId, label: resolvedLabel, variant: resolvedVariant),
        x: x,
        y: y,
        label: resolvedLabel.isEmpty ? resolvedId : resolvedLabel,
        variant: resolvedVariant
      )
    }
  }

  func move_overlay_cursor(
    cursor_id: RustString,
    x: Double,
    y: Double,
    label: RustString,
    variant: RustString,
    duration_ms: UInt64
  ) -> NativeActionResponse {
    runOnMain {
      let resolvedId = self.normalizeCursorId(cursor_id.toString(), fallback: "auv")
      let resolvedLabel = label.toString()
      let resolvedVariant = self.variantFrom(variant.toString())
      let targetLabel = resolvedLabel.isEmpty ? resolvedId : resolvedLabel
      let state = self.ensureCursor(id: resolvedId, label: targetLabel, variant: resolvedVariant)
      let startPoint = self.cursorStartPoint(state)
      self.moveCursor(
        state: state,
        targetX: x,
        targetY: y,
        label: targetLabel,
        variant: resolvedVariant,
        duration: max(0.0, Double(duration_ms) / 1000.0),
        startPoint: startPoint
      )
    }
  }

  func flash_overlay_cursor_id(
    cursor_id: RustString,
    x: Double,
    y: Double,
    label: RustString,
    duration_ms: UInt64
  ) -> NativeActionResponse {
    runOnMain {
      let resolvedId = self.normalizeCursorId(cursor_id.toString(), fallback: "auv")
      self.flashCursor(
        id: resolvedId,
        x: x,
        y: y,
        label: label.toString(),
        durationMs: duration_ms
      )
    }
  }

  func flash_overlay_cursor(
    x: Double,
    y: Double,
    label: RustString,
    duration_ms: UInt64
  ) -> NativeActionResponse {
    runOnMain {
      self.flashCursor(
        id: "auv",
        x: x,
        y: y,
        label: label.toString(),
        durationMs: duration_ms
      )
    }
  }

  func hide_overlay_cursor_id(cursor_id: RustString) -> NativeActionResponse {
    runOnMain {
      self.hideCursor(id: self.normalizeCursorId(cursor_id.toString(), fallback: "auv"))
    }
  }

  func hide_overlay_cursor() -> NativeActionResponse {
    runOnMain {
      self.stopUserCursorTracking()
      for state in self.cursors.values {
        state.window.orderOut(nil)
      }
    }
  }

  func shutdown_overlay_cursor() -> NativeActionResponse {
    runOnMain {
      self.stopUserCursorTracking()
      for state in self.cursors.values {
        state.window.orderOut(nil)
        state.window.close()
      }
      self.cursors.removeAll()
    }
  }

  private func ensureCursor(id: String, label: String, variant: AuvOverlayCursorVariant) -> NativeOverlayCursorState {
    if let state = cursors[id] {
      return state
    }
    let (window, view) = makeCursorWindow()
    let state = NativeOverlayCursorState(
      window: window,
      view: view,
      label: label.isEmpty ? id : label,
      variant: variant
    )
    cursors[id] = state
    return state
  }

  private func makeCursorWindow() -> (NSWindow, NativeOverlayCursorView) {
    let app = NSApplication.shared
    app.setActivationPolicy(.accessory)

    let initial = NSSize(width: 160, height: 24)
    let view = NativeOverlayCursorView(frame: NSRect(origin: .zero, size: initial))
    let window = NSWindow(
      contentRect: NSRect(origin: .zero, size: initial),
      styleMask: [.borderless],
      backing: .buffered,
      defer: false
    )
    window.contentView = view
    window.isOpaque = false
    window.backgroundColor = .clear
    window.ignoresMouseEvents = true
    window.hasShadow = false
    window.level = .floating
    window.isReleasedWhenClosed = false

    return (window, view)
  }

  private func placeCursor(
    state: NativeOverlayCursorState,
    x: Double,
    y: Double,
    label: String,
    variant: AuvOverlayCursorVariant
  ) {
    let window = state.window
    let view = state.view
    view.label = label
    view.variant = variant
    state.logicalX = x
    state.logicalY = y
    state.label = label
    state.variant = variant

    // Resize host window to fit sprite + dynamically-sized label.
    let intrinsic = view.intrinsicLayoutSize()
    let viewSize = NSSize(
      width: ceil(intrinsic.width),
      height: ceil(intrinsic.height)
    )
    view.frame = NSRect(origin: .zero, size: viewSize)
    view.needsDisplay = true

    // Position the sprite tip a few px down-right of the requested
    // target point — matches the hover offset in the design preview.
    let offsetX = 4.0
    let offsetY = 4.0
    let topLeftX = x + offsetX
    let topLeftY = y + offsetY
    let appKitY = referenceHeight() - topLeftY - Double(viewSize.height)
    let frame = NSRect(
      x: topLeftX,
      y: appKitY,
      width: Double(viewSize.width),
      height: Double(viewSize.height)
    )

    window.setFrame(frame, display: true)
    window.orderFrontRegardless()
    window.displayIfNeeded()
  }

  private func moveCursor(
    state: NativeOverlayCursorState,
    targetX: Double,
    targetY: Double,
    label: String,
    variant: AuvOverlayCursorVariant,
    duration: Double,
    startPoint: (x: Double, y: Double)
  ) {
    if duration <= 0 {
      placeCursor(
        state: state,
        x: targetX,
        y: targetY,
        label: label,
        variant: variant
      )
      return
    }

    let start = Date()
    let deadline = start.addingTimeInterval(duration)
    while true {
      let elapsed = Date().timeIntervalSince(start)
      let rawT = min(1.0, max(0.0, elapsed / duration))
      let eased = 1.0 - pow(1.0 - rawT, 3.0)
      let currentX = startPoint.x + (targetX - startPoint.x) * eased
      let currentY = startPoint.y + (targetY - startPoint.y) * eased
      placeCursor(
        state: state,
        x: currentX,
        y: currentY,
        label: label,
        variant: variant
      )
      drainEvents(until: Date().addingTimeInterval(1.0 / 60.0))
      if Date() >= deadline || rawT >= 1.0 {
        break
      }
    }

    placeCursor(
      state: state,
      x: targetX,
      y: targetY,
      label: label,
      variant: variant
    )
  }

  private func flashCursor(id: String, x: Double, y: Double, label: String, durationMs: UInt64) {
    let state = ensureCursor(id: id, label: label, variant: .auvClick)
    let previousLabel = state.label
    let previousVariant = state.variant
    let targetLabel = label.isEmpty ? "auv · click" : label
    placeCursor(
      state: state,
      x: x,
      y: y,
      label: targetLabel,
      variant: .auvClick
    )
    drainEvents(until: Date().addingTimeInterval(Double(durationMs) / 1000.0))
    placeCursor(
      state: state,
      x: x,
      y: y,
      label: previousLabel.isEmpty ? id : previousLabel,
      variant: previousVariant == .auvClick ? .auv : previousVariant
    )
  }

  private func hideCursor(id: String) {
    if id == "you" {
      stopUserCursorTracking()
    }
    cursors[id]?.window.orderOut(nil)
  }

  private func startUserCursorTracking(label: String) {
    userCursorTrackingLabel = label.isEmpty ? "you" : label
    updateUserCursorFromHardware()

    if userCursorTrackingTimer != nil {
      return
    }

    let timer = Timer(timeInterval: 1.0 / 30.0, repeats: true) { [weak self] _ in
      self?.updateUserCursorFromHardware()
    }
    userCursorTrackingTimer = timer
    RunLoop.main.add(timer, forMode: .common)
  }

  private func stopUserCursorTracking() {
    userCursorTrackingTimer?.invalidate()
    userCursorTrackingTimer = nil
  }

  private func updateUserCursorFromHardware() {
    let userPoint = currentMouseLogicalPoint()
    placeCursor(
      state: ensureCursor(id: "you", label: userCursorTrackingLabel, variant: .you),
      x: userPoint.x,
      y: userPoint.y,
      label: userCursorTrackingLabel,
      variant: .you
    )
  }

  private func cursorStartPoint(_ state: NativeOverlayCursorState) -> (x: Double, y: Double) {
    if let x = state.logicalX, let y = state.logicalY {
      return (x, y)
    }
    return currentMouseLogicalPoint()
  }

  private func normalizeCursorId(_ raw: String, fallback: String) -> String {
    let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
    return trimmed.isEmpty ? fallback : trimmed
  }

  private func variantFrom(_ raw: String) -> AuvOverlayCursorVariant {
    switch raw.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() {
    case "you", "user", "human":
      return .you
    case "auv-click", "auv_click", "click", "auvclick":
      return .auvClick
    default:
      return .auv
    }
  }

  private func currentMouseLogicalPoint() -> (x: Double, y: Double) {
    // NSEvent.mouseLocation is in AppKit global coordinates, with a
    // bottom-left origin on the main display. Convert it to the same
    // top-left global-logical space accepted by show_overlay_cursor.
    let location = NSEvent.mouseLocation
    return (
      x: Double(location.x),
      y: referenceHeight() - Double(location.y)
    )
  }

  private func referenceHeight() -> Double {
    Double(NSScreen.main?.frame.height ?? NSScreen.screens.first?.frame.height ?? 0)
  }

  private func drainEvents(until deadline: Date) {
    while Date() < deadline {
      autoreleasepool {
        if let event = NSApplication.shared.nextEvent(
          matching: .any,
          until: Date().addingTimeInterval(0.005),
          inMode: .default,
          dequeue: true
        ) {
          NSApplication.shared.sendEvent(event)
        }
        NSApplication.shared.updateWindows()
      }
    }
  }
}

func make_overlay_controller() -> NativeOverlayController {
  NativeOverlayController()
}

func pump_overlay_events(duration_ms: UInt64) -> NativeActionResponse {
  runOnMain {
    let deadline = Date().addingTimeInterval(Double(duration_ms) / 1000.0)
    while Date() < deadline {
      autoreleasepool {
        if let event = NSApplication.shared.nextEvent(
          matching: .any,
          until: Date().addingTimeInterval(0.01),
          inMode: .default,
          dequeue: true
        ) {
          NSApplication.shared.sendEvent(event)
        }
        NSApplication.shared.updateWindows()
      }
    }
  }
}

private func runOnMain(_ body: @escaping () -> Void) -> NativeActionResponse {
  if Thread.isMainThread {
    body()
    return nativeActionOk()
  }

  var result = nativeActionOk()
  DispatchQueue.main.sync {
    body()
    result = nativeActionOk()
  }
  return result
}

// MARK: - Pixel sprites ported from auv-design-system assets/
//
// Each function returns the rect data verbatim from the matching
// SVG: cursor-auv.svg (replay state) and cursor-you.svg (illustration).
// Coordinates are in 2-unit cells on a 24-unit grid — see the SVG
// `viewBox="0 0 24 24"`.

private let auvOutlineColor = NSColor(srgbRed: 0.082, green: 0.090, blue: 0.102, alpha: 1.0)  // #15171a
private let auvBodyColor = NSColor(srgbRed: 0.0, green: 0.769, blue: 0.823, alpha: 1.0)        // #00c4d2
private let auvHighlightColor = NSColor(srgbRed: 0.0, green: 0.878, blue: 0.878, alpha: 1.0)   // #00e0e0
private let auvClickHighlightColor = NSColor(srgbRed: 0.812, green: 0.957, blue: 0.969, alpha: 1.0) // #cff4f7
private let auvAccentColor = NSColor(srgbRed: 0.498, green: 0.816, blue: 0.188, alpha: 1.0)    // #7fd030
private let auvSparkColor = NSColor(srgbRed: 0.627, green: 0.878, blue: 0.125, alpha: 1.0)      // #a0e020

private let youOutlineColor = NSColor(srgbRed: 0.055, green: 0.063, blue: 0.075, alpha: 1.0)   // #0e1013
private let youBodyColor = NSColor(srgbRed: 0.353, green: 0.384, blue: 0.439, alpha: 1.0)      // #5a6270
private let youHighlightColor = NSColor(srgbRed: 0.604, green: 0.639, blue: 0.698, alpha: 1.0) // #9aa3b2

private func auvSprite() -> [AuvPixelCell] {
  var cells: [AuvPixelCell] = []
  // outline (#15171a)
  let outline: [(Int, Int, Int, Int)] = [
    (0, 0, 2, 2), (0, 2, 2, 2), (0, 4, 2, 2), (0, 6, 2, 2), (0, 8, 2, 2),
    (0, 10, 2, 2), (0, 12, 2, 2), (0, 14, 2, 2), (0, 16, 2, 2),
    (2, 2, 2, 2), (2, 16, 2, 2),
    (4, 4, 2, 2), (4, 16, 2, 2),
    (6, 6, 2, 2), (6, 14, 2, 2),
    (8, 8, 2, 2), (8, 12, 2, 2),
    (10, 10, 2, 2), (10, 14, 2, 2),
    (12, 10, 2, 2), (12, 14, 2, 2),
    (14, 14, 2, 2), (14, 16, 2, 2),
  ]
  for (x, y, w, h) in outline {
    cells.append(AuvPixelCell(x: x, y: y, width: w, height: h, color: auvOutlineColor))
  }
  // body (#00c4d2)
  let body: [(Int, Int, Int, Int)] = [
    (2, 4, 2, 12), (4, 6, 2, 10), (6, 8, 2, 6), (8, 10, 2, 2),
  ]
  for (x, y, w, h) in body {
    cells.append(AuvPixelCell(x: x, y: y, width: w, height: h, color: auvBodyColor))
  }
  // highlight (#00e0e0)
  let highlight: [(Int, Int, Int, Int)] = [
    (2, 4, 2, 2), (2, 6, 2, 2),
  ]
  for (x, y, w, h) in highlight {
    cells.append(AuvPixelCell(x: x, y: y, width: w, height: h, color: auvHighlightColor))
  }
  // crook accent (#7fd030)
  let accent: [(Int, Int, Int, Int)] = [
    (10, 12, 2, 2), (12, 12, 2, 2),
  ]
  for (x, y, w, h) in accent {
    cells.append(AuvPixelCell(x: x, y: y, width: w, height: h, color: auvAccentColor))
  }
  return cells
}

private func auvClickSprite() -> [AuvPixelCell] {
  var cells: [AuvPixelCell] = []
  // click burst (#7fd030)
  let burstAccent: [(Int, Int, Int, Int)] = [
    (6, -2, 2, 2), (14, 2, 2, 2), (-2, 6, 2, 2), (-4, 12, 2, 2), (16, 10, 2, 2),
  ]
  for (x, y, w, h) in burstAccent {
    cells.append(AuvPixelCell(x: x, y: y, width: w, height: h, color: auvAccentColor))
  }
  // click burst highlight (#a0e020)
  let burstHighlight: [(Int, Int, Int, Int)] = [
    (6, 0, 2, 2), (12, 0, 2, 2), (14, 6, 2, 2), (-2, 10, 2, 2),
  ]
  for (x, y, w, h) in burstHighlight {
    cells.append(AuvPixelCell(x: x, y: y, width: w, height: h, color: auvSparkColor))
  }
  // outline (#15171a)
  let outline: [(Int, Int, Int, Int)] = [
    (0, 0, 2, 2), (0, 2, 2, 2), (0, 4, 2, 2), (0, 6, 2, 2), (0, 8, 2, 2),
    (0, 10, 2, 2), (0, 12, 2, 2), (0, 14, 2, 2), (0, 16, 2, 2),
    (2, 2, 2, 2), (2, 16, 2, 2),
    (4, 4, 2, 2), (4, 16, 2, 2),
    (6, 6, 2, 2), (6, 14, 2, 2),
    (8, 8, 2, 2), (8, 12, 2, 2),
    (10, 10, 2, 2), (10, 14, 2, 2),
    (12, 10, 2, 2), (12, 14, 2, 2),
    (14, 14, 2, 2), (14, 16, 2, 2),
  ]
  for (x, y, w, h) in outline {
    cells.append(AuvPixelCell(x: x, y: y, width: w, height: h, color: auvOutlineColor))
  }
  // click highlight (#cff4f7)
  let highlight: [(Int, Int, Int, Int)] = [
    (2, 4, 2, 4), (4, 6, 2, 4),
  ]
  for (x, y, w, h) in highlight {
    cells.append(AuvPixelCell(x: x, y: y, width: w, height: h, color: auvClickHighlightColor))
  }
  // body (#00c4d2)
  let body: [(Int, Int, Int, Int)] = [
    (2, 8, 2, 8), (4, 10, 2, 6), (6, 8, 2, 6), (8, 10, 2, 2),
  ]
  for (x, y, w, h) in body {
    cells.append(AuvPixelCell(x: x, y: y, width: w, height: h, color: auvBodyColor))
  }
  // crook accent (#7fd030)
  let accent: [(Int, Int, Int, Int)] = [
    (10, 12, 2, 2), (12, 12, 2, 2),
  ]
  for (x, y, w, h) in accent {
    cells.append(AuvPixelCell(x: x, y: y, width: w, height: h, color: auvAccentColor))
  }
  return cells
}

private func youSprite() -> [AuvPixelCell] {
  var cells: [AuvPixelCell] = []
  // outline (#0e1013)
  let outline: [(Int, Int, Int, Int)] = [
    (0, 0, 2, 2), (0, 2, 2, 2), (0, 4, 2, 2), (0, 6, 2, 2), (0, 8, 2, 2),
    (0, 10, 2, 2), (0, 12, 2, 2), (0, 14, 2, 2), (0, 16, 2, 2),
    (2, 2, 2, 2), (2, 16, 2, 2),
    (4, 4, 2, 2), (4, 16, 2, 2),
    (6, 6, 2, 2), (6, 14, 2, 2),
    (8, 8, 2, 2), (8, 12, 2, 2),
    (10, 10, 2, 2), (10, 14, 2, 2),
    (12, 10, 2, 2), (12, 14, 2, 2),
    (14, 14, 2, 2), (14, 16, 2, 2),
  ]
  for (x, y, w, h) in outline {
    cells.append(AuvPixelCell(x: x, y: y, width: w, height: h, color: youOutlineColor))
  }
  // body (#5a6270)
  let body: [(Int, Int, Int, Int)] = [
    (2, 4, 2, 12), (4, 6, 2, 10), (6, 8, 2, 6), (8, 10, 2, 2),
  ]
  for (x, y, w, h) in body {
    cells.append(AuvPixelCell(x: x, y: y, width: w, height: h, color: youBodyColor))
  }
  // highlight (#9aa3b2)
  let highlight: [(Int, Int, Int, Int)] = [
    (2, 4, 2, 2), (2, 6, 2, 2),
  ]
  for (x, y, w, h) in highlight {
    cells.append(AuvPixelCell(x: x, y: y, width: w, height: h, color: youHighlightColor))
  }
  return cells
}
