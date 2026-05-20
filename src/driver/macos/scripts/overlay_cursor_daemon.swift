import AppKit
import Foundation

struct IncomingCommand: Decodable {
  let type: String
  let x: Double?
  let y: Double?
  let label: String?
}

func emitAck(ok: Bool, event: String, error: String? = nil) {
  var payload: [String: Any] = [
    "ok": ok,
    "event": event
  ]
  if let error {
    payload["error"] = error
  }

  do {
    let data = try JSONSerialization.data(withJSONObject: payload, options: [])
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write(Data("\n".utf8))
  } catch {
    let fallback = "{\"ok\":false,\"event\":\"ack_failed\",\"error\":\"\(error.localizedDescription)\"}\n"
    FileHandle.standardOutput.write(Data(fallback.utf8))
  }
}

final class CursorOverlayView: NSView {
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

final class OverlayController {
  private let size = NSSize(width: 120, height: 48)
  private var window: NSWindow?
  private var overlayView: CursorOverlayView?

  func showCursor(x: Double, y: Double, label: String) {
    let window = ensureWindow()
    overlayView?.label = label
    overlayView?.needsDisplay = true

    let offsetX = 12.0
    let offsetY = 12.0
    let topLeftX = x + offsetX
    let topLeftY = y + offsetY
    let appKitY = referenceHeight() - topLeftY - size.height
    let frame = NSRect(
      x: topLeftX,
      y: appKitY,
      width: size.width,
      height: size.height
    )

    window.setFrame(frame, display: true)
    window.orderFrontRegardless()
  }

  func hideCursor() {
    window?.orderOut(nil)
  }

  private func ensureWindow() -> NSWindow {
    if let window {
      return window
    }

    let view = CursorOverlayView(frame: NSRect(origin: .zero, size: size))
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

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let controller = OverlayController()

DispatchQueue.global(qos: .userInitiated).async {
  let decoder = JSONDecoder()

  while let line = readLine() {
    guard let data = line.data(using: .utf8) else {
      emitAck(ok: false, event: "decode_failed", error: "stdin line is not utf8")
      continue
    }

    do {
      let command = try decoder.decode(IncomingCommand.self, from: data)
      DispatchQueue.main.async {
        switch command.type {
        case "show_cursor":
          guard let x = command.x, let y = command.y else {
            emitAck(ok: false, event: "show_failed", error: "show_cursor requires x and y")
            return
          }
          controller.showCursor(x: x, y: y, label: command.label ?? "AUV")
          emitAck(ok: true, event: "shown")
        case "hide_cursor":
          controller.hideCursor()
          emitAck(ok: true, event: "hidden")
        case "shutdown":
          controller.hideCursor()
          emitAck(ok: true, event: "shutdown")
          app.terminate(nil)
        default:
          emitAck(
            ok: false,
            event: "unknown_command",
            error: "unsupported overlay command \(command.type)"
          )
        }
      }
    } catch {
      emitAck(ok: false, event: "decode_failed", error: error.localizedDescription)
    }
  }

  DispatchQueue.main.async {
    app.terminate(nil)
  }
}

app.run()
