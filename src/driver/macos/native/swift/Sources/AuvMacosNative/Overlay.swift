import AppKit
import Foundation

final class NativeOverlayCursorView: NSView {
  var label: String = "AUV"

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

final class NativeOverlayController {
  private let size = NSSize(width: 120, height: 48)
  private var window: NSWindow?
  private var overlayView: NativeOverlayCursorView?

  func show_overlay_cursor(x: Double, y: Double, label: RustString) -> NativeActionResponse {
    runOnMain {
      let window = self.ensureWindow()
      self.overlayView?.label = label.toString()
      self.overlayView?.needsDisplay = true

      let offsetX = 12.0
      let offsetY = 12.0
      let topLeftX = x + offsetX
      let topLeftY = y + offsetY
      let appKitY = self.referenceHeight() - topLeftY - self.size.height
      let frame = NSRect(
        x: topLeftX,
        y: appKitY,
        width: self.size.width,
        height: self.size.height
      )

      window.setFrame(frame, display: true)
      window.orderFrontRegardless()
      window.displayIfNeeded()
    }
  }

  func hide_overlay_cursor() -> NativeActionResponse {
    runOnMain {
      self.window?.orderOut(nil)
    }
  }

  func shutdown_overlay_cursor() -> NativeActionResponse {
    runOnMain {
      self.window?.orderOut(nil)
      self.window?.close()
      self.window = nil
      self.overlayView = nil
    }
  }

  private func ensureWindow() -> NSWindow {
    if let window {
      return window
    }

    let app = NSApplication.shared
    app.setActivationPolicy(.accessory)

    let view = NativeOverlayCursorView(frame: NSRect(origin: .zero, size: size))
    let window = NSWindow(
      contentRect: NSRect(origin: .zero, size: size),
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

    self.window = window
    self.overlayView = view
    return window
  }

  private func referenceHeight() -> Double {
    Double(NSScreen.main?.frame.height ?? NSScreen.screens.first?.frame.height ?? 0)
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
