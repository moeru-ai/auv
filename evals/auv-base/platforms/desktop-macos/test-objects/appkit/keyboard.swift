import AppKit

// Independent receiver: records OS-delivered events and the NSTextView's content.
let directory = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
let eventsURL = directory.appendingPathComponent("events.jsonl")
FileManager.default.createFile(atPath: eventsURL.path, contents: nil)

let app = NSApplication.shared
app.setActivationPolicy(.regular)
let previousApp = NSWorkspace.shared.frontmostApplication

let window = NSWindow(
  contentRect: NSRect(x: 100, y: 100, width: 400, height: 240),
  styleMask: [.titled, .closable],
  backing: .buffered,
  defer: false
)
window.title = "AUV keyboard receiver"

let textView = NSTextView(frame: NSRect(x: 0, y: 0, width: 400, height: 240))
textView.isRichText = false

window.contentView = textView
window.makeKeyAndOrderFront(nil)
window.makeFirstResponder(textView)

let monitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .keyUp, .flagsChanged]) { event in
  let record: [String: Any] = [
    "type": event.type.rawValue,
    "key": event.keyCode,
    "flags": event.modifierFlags.rawValue,
    "characters": event.type == .flagsChanged ? "" : (event.characters ?? ""),
    "window": event.windowNumber,
    "active": NSApp.isActive,
  ]

  if let data = try? JSONSerialization.data(withJSONObject: record),
     let file = try? FileHandle(forWritingTo: eventsURL)
  {
    file.seekToEndOfFile()
    file.write(data)
    file.write(Data([10]))
    try? file.close()
  }

  return event
}

// Give the receiver a key window, then return application focus to the prior app.
// The test must observe background receipt, not only target delivery while active.
DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
  previousApp?.activate(options: [])
  try! Data("ready".utf8).write(to: directory.appendingPathComponent("ready"))
}

Timer.scheduledTimer(withTimeInterval: 0.05, repeats: true) { _ in
  try? Data(textView.string.utf8).write(to: directory.appendingPathComponent("text"), options: .atomic)
}

// Bound the receiver even if the sender terminates before its child cleanup.
DispatchQueue.main.asyncAfter(deadline: .now() + 30) { NSApp.terminate(nil) }

app.run()
