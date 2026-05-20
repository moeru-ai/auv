import AppKit
import CoreGraphics
import Foundation

struct Options {
  var targetX: Double?
  var targetY: Double?
  var deltaX: Double = 420
  var deltaY: Double = 0
  var preClickMs: UInt32 = 350
  var postClickMs: UInt32 = 450
  var label: String = "AUV"
}

func usage() -> Never {
  fputs(
    """
    usage: swift scripts/local/route-b-click-wrapper-smoke.swift [options]

    Options:
      --target-x <number>     Absolute global logical click target x.
      --target-y <number>     Absolute global logical click target y.
      --delta-x <number>      Target x offset from current cursor when no absolute target is supplied. Default: 420.
      --delta-y <number>      Target y offset from current cursor when no absolute target is supplied. Default: 0.
      --pre-click-ms <ms>     Time to show the virtual cursor before clicking. Default: 350.
      --post-click-ms <ms>    Time to keep the evidence window after clicking. Default: 450.
      --label <text>          Virtual cursor label. Default: AUV.

    This smoke creates its own small click target window, shows a visual-only cursor overlay,
    performs CGEvent click at the target, restores the real cursor, and reports whether the
    target view received the click. It does not integrate with AUV recipes or runtime behavior.

    """,
    stderr
  )
  exit(64)
}

func readNumber(_ args: [String], _ index: inout Int, _ flag: String) -> Double {
  guard index + 1 < args.count, let value = Double(args[index + 1]) else {
    usage()
  }
  index += 2
  return value
}

func readUInt(_ args: [String], _ index: inout Int, _ flag: String) -> UInt32 {
  let value = readNumber(args, &index, flag)
  guard value >= 0 else {
    usage()
  }
  return UInt32(value.rounded())
}

func readString(_ args: [String], _ index: inout Int) -> String {
  guard index + 1 < args.count else {
    usage()
  }
  index += 2
  return args[index - 1]
}

func parseOptions() -> Options {
  var options = Options()
  let args = Array(CommandLine.arguments.dropFirst())
  var index = 0

  while index < args.count {
    switch args[index] {
    case "--target-x":
      options.targetX = readNumber(args, &index, args[index])
    case "--target-y":
      options.targetY = readNumber(args, &index, args[index])
    case "--delta-x":
      options.deltaX = readNumber(args, &index, args[index])
    case "--delta-y":
      options.deltaY = readNumber(args, &index, args[index])
    case "--pre-click-ms":
      options.preClickMs = readUInt(args, &index, args[index])
    case "--post-click-ms":
      options.postClickMs = readUInt(args, &index, args[index])
    case "--label":
      options.label = readString(args, &index)
    case "--help", "-h":
      usage()
    default:
      usage()
    }
  }

  if (options.targetX == nil) != (options.targetY == nil) {
    usage()
  }

  return options
}

func sleepMs(_ milliseconds: UInt32) {
  if milliseconds > 0 {
    usleep(milliseconds * 1000)
  }
}

func referenceHeight() -> Double {
  Double(NSScreen.main?.frame.height ?? NSScreen.screens.first?.frame.height ?? 0)
}

func appKitFrame(topLeftX: Double, topLeftY: Double, width: Double, height: Double) -> NSRect {
  NSRect(
    x: topLeftX,
    y: referenceHeight() - topLeftY - height,
    width: width,
    height: height
  )
}

func pointText(_ point: CGPoint) -> String {
  "\(String(format: "%.3f", point.x)),\(String(format: "%.3f", point.y))"
}

final class ClickTargetView: NSView {
  var clickCount = 0
  var lastClick: NSPoint?

  override var isFlipped: Bool {
    true
  }

  override func mouseDown(with event: NSEvent) {
    clickCount += 1
    lastClick = event.locationInWindow
    needsDisplay = true
  }

  override func draw(_ dirtyRect: NSRect) {
    super.draw(dirtyRect)

    NSColor(calibratedWhite: 0.08, alpha: 0.94).setFill()
    bounds.fill()

    let targetRect = bounds.insetBy(dx: 28, dy: 22)
    let targetPath = NSBezierPath(roundedRect: targetRect, xRadius: 14, yRadius: 14)
    (clickCount > 0 ? NSColor.systemGreen : NSColor.systemOrange).setFill()
    targetPath.fill()

    let paragraph = NSMutableParagraphStyle()
    paragraph.alignment = .center
    let title = clickCount > 0 ? "clicked" : "click target"
    let attributes: [NSAttributedString.Key: Any] = [
      .font: NSFont.systemFont(ofSize: 15, weight: .semibold),
      .foregroundColor: NSColor.white,
      .paragraphStyle: paragraph
    ]
    (title as NSString).draw(
      in: targetRect.insetBy(dx: 8, dy: 18),
      withAttributes: attributes
    )
  }
}

final class CursorOverlayView: NSView {
  var label = "AUV"

  override var isFlipped: Bool {
    true
  }

  override func draw(_ dirtyRect: NSRect) {
    super.draw(dirtyRect)

    let cursorPath = NSBezierPath()
    cursorPath.move(to: NSPoint(x: 8, y: 6))
    cursorPath.line(to: NSPoint(x: 8, y: 32))
    cursorPath.line(to: NSPoint(x: 18, y: 24))
    cursorPath.line(to: NSPoint(x: 24, y: 39))
    cursorPath.line(to: NSPoint(x: 31, y: 36))
    cursorPath.line(to: NSPoint(x: 25, y: 21))
    cursorPath.line(to: NSPoint(x: 38, y: 21))
    cursorPath.close()
    NSColor.white.setFill()
    cursorPath.fill()
    NSColor.black.withAlphaComponent(0.72).setStroke()
    cursorPath.lineWidth = 1.5
    cursorPath.stroke()

    let labelRect = NSRect(x: 42, y: 10, width: 72, height: 26)
    let background = NSBezierPath(roundedRect: labelRect, xRadius: 9, yRadius: 9)
    NSColor.black.withAlphaComponent(0.82).setFill()
    background.fill()

    let paragraph = NSMutableParagraphStyle()
    paragraph.alignment = .center
    let attributes: [NSAttributedString.Key: Any] = [
      .font: NSFont.systemFont(ofSize: 13, weight: .semibold),
      .foregroundColor: NSColor.white,
      .paragraphStyle: paragraph
    ]
    (label as NSString).draw(
      in: labelRect.insetBy(dx: 8, dy: 5),
      withAttributes: attributes
    )
  }
}

func makeWindow(frame: NSRect, level: NSWindow.Level, ignoresMouseEvents: Bool) -> NSWindow {
  let window = NSWindow(
    contentRect: frame,
    styleMask: [.borderless],
    backing: .buffered,
    defer: false
  )
  window.isOpaque = false
  window.backgroundColor = .clear
  window.hasShadow = false
  window.level = level
  window.ignoresMouseEvents = ignoresMouseEvents
  window.isReleasedWhenClosed = false
  return window
}

func postLeftClick(at point: CGPoint) {
  guard
    let down = CGEvent(
      mouseEventSource: nil,
      mouseType: .leftMouseDown,
      mouseCursorPosition: point,
      mouseButton: .left
    ),
    let up = CGEvent(
      mouseEventSource: nil,
      mouseType: .leftMouseUp,
      mouseCursorPosition: point,
      mouseButton: .left
    )
  else {
    return
  }

  down.setIntegerValueField(.mouseEventClickState, value: 1)
  up.setIntegerValueField(.mouseEventClickState, value: 1)
  down.post(tap: .cghidEventTap)
  up.post(tap: .cghidEventTap)
}

let options = parseOptions()
let initialCursor = CGEvent(source: nil)?.location ?? CGPoint(x: 0, y: 0)
let clickTarget = CGPoint(
  x: options.targetX ?? (initialCursor.x + options.deltaX),
  y: options.targetY ?? (initialCursor.y + options.deltaY)
)

let targetSize = NSSize(width: 190, height: 104)
let targetFrame = appKitFrame(
  topLeftX: clickTarget.x - targetSize.width / 2,
  topLeftY: clickTarget.y - targetSize.height / 2,
  width: targetSize.width,
  height: targetSize.height
)
let overlaySize = NSSize(width: 120, height: 48)
let overlayFrame = appKitFrame(
  topLeftX: clickTarget.x + 12,
  topLeftY: clickTarget.y + 12,
  width: overlaySize.width,
  height: overlaySize.height
)

let app = NSApplication.shared
app.setActivationPolicy(.accessory)

let targetView = ClickTargetView(frame: NSRect(origin: .zero, size: targetSize))
let targetWindow = makeWindow(frame: targetFrame, level: .floating, ignoresMouseEvents: false)
targetWindow.contentView = targetView
targetWindow.orderFrontRegardless()

let overlayView = CursorOverlayView(frame: NSRect(origin: .zero, size: overlaySize))
overlayView.label = options.label
let overlayWindow = makeWindow(
  frame: overlayFrame,
  level: NSWindow.Level(rawValue: NSWindow.Level.floating.rawValue + 1),
  ignoresMouseEvents: true
)
overlayWindow.contentView = overlayView
overlayWindow.orderFrontRegardless()

print("routeBClickWrapperSmoke=true")
print("initialCursor=\(pointText(initialCursor))")
print("clickTarget=\(pointText(clickTarget))")
print("overlayIgnoresMouseEvents=true")
print("targetWindowFrame=\(String(format: "%.3f", targetFrame.origin.x)),\(String(format: "%.3f", targetFrame.origin.y)),\(String(format: "%.3f", targetFrame.size.width)),\(String(format: "%.3f", targetFrame.size.height))")
print("preClickMs=\(options.preClickMs)")
print("postClickMs=\(options.postClickMs)")
print("manualObservation=watch whether the real cursor visibly flashes at target while the virtual AUV cursor remains visible")
fflush(stdout)

DispatchQueue.main.asyncAfter(deadline: .now() + .milliseconds(Int(options.preClickMs))) {
  let savedCursor = CGEvent(source: nil)?.location ?? initialCursor
  let started = DispatchTime.now().uptimeNanoseconds
  CGWarpMouseCursorPosition(clickTarget)
  postLeftClick(at: clickTarget)
  CGWarpMouseCursorPosition(savedCursor)
  let ended = DispatchTime.now().uptimeNanoseconds
  let elapsedMs = Double(ended - started) / 1_000_000.0

  DispatchQueue.main.asyncAfter(deadline: .now() + .milliseconds(Int(options.postClickMs))) {
    let finalCursor = CGEvent(source: nil)?.location ?? savedCursor
    let restored = abs(finalCursor.x - savedCursor.x) < 1.0 && abs(finalCursor.y - savedCursor.y) < 1.0
    print("savedCursor=\(pointText(savedCursor))")
    print("finalCursor=\(pointText(finalCursor))")
    print("restored=\(restored)")
    print("clickDelivered=\(targetView.clickCount > 0)")
    print("clickCount=\(targetView.clickCount)")
    if let lastClick = targetView.lastClick {
      print("targetWindowClickLocation=\(String(format: "%.3f", lastClick.x)),\(String(format: "%.3f", lastClick.y))")
    }
    print("warpClickRestoreElapsedMs=\(String(format: "%.3f", elapsedMs))")
    fflush(stdout)
    overlayWindow.orderOut(nil)
    targetWindow.orderOut(nil)
    app.terminate(nil)
  }
}

app.run()
