import AppKit

// Independent receiver: records AppKit events, never driver return values.
let directory = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
let eventsURL = directory.appendingPathComponent("events.jsonl")
FileManager.default.createFile(atPath: eventsURL.path, contents: nil)

final class Receiver: NSView {
  override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
  override func mouseDown(with event: NSEvent) {}
  override func mouseUp(with event: NSEvent) {}
  override func rightMouseDown(with event: NSEvent) {}
  override func rightMouseUp(with event: NSEvent) {}
}

let app = NSApplication.shared
app.setActivationPolicy(.regular)
let window = NSWindow(
  contentRect: NSRect(x: 100, y: 100, width: 360, height: 240),
  styleMask: [.titled, .closable], backing: .buffered, defer: false
)
window.title = "AUV ClickModifiers receiver"
window.contentView = Receiver(frame: NSRect(x: 0, y: 0, width: 360, height: 240))
let monitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .leftMouseUp, .rightMouseDown, .rightMouseUp]) { event in
  guard event.windowNumber == window.windowNumber else { return event }
  let point = event.locationInWindow
  // Ignore the compatibility route's off-window primer events.
  guard window.contentView!.bounds.contains(point) else { return event }
  let record: [String: Any] = [
    "type": event.type.rawValue,
    "flags": event.modifierFlags.rawValue,
    "count": event.clickCount,
    "window": event.windowNumber,
    "active": NSApp.isActive,
  ]
  if let data = try? JSONSerialization.data(withJSONObject: record),
     let file = try? FileHandle(forWritingTo: eventsURL) {
    file.seekToEndOfFile()
    file.write(data)
    file.write(Data([10]))
    try? file.close()
  }
  return event
}
window.makeKeyAndOrderFront(nil)
try Data("ready".utf8).write(to: directory.appendingPathComponent("ready"))
app.run()
