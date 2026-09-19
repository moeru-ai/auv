import AppKit
import Darwin

setbuf(stdout, nil)
final class Receiver: NSView {
  override var acceptsFirstResponder: Bool { true }
  override func mouseDown(with event: NSEvent) { print("down") }
  override func mouseDragged(with event: NSEvent) { print("drag") }
  override func mouseUp(with event: NSEvent) { print("up") }
}
let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let window = NSWindow(contentRect: NSRect(x: 100, y: 100, width: 320, height: 220),
  styleMask: [.titled], backing: .buffered, defer: false)
window.title = "AUV held mouse receiver"
window.contentView = Receiver(frame: NSRect(x: 0, y: 0, width: 320, height: 220))
window.orderFrontRegardless()
print("ready")
// A failed sender must not leave the test receiver running indefinitely.
DispatchQueue.main.asyncAfter(deadline: .now() + 15) { app.terminate(nil) }
app.run()
