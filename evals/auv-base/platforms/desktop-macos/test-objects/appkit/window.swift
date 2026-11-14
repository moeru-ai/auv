import AppKit

// Throwaway window for the window management evaluation: resizable and miniaturizable, so
// set_frame, minimize and maximize all apply. It holds no state; the driver reads it back.
let directory = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)

let app = NSApplication.shared
app.setActivationPolicy(.regular)
let previousApp = NSWorkspace.shared.frontmostApplication

let window = NSWindow(
  contentRect: NSRect(x: 200, y: 200, width: 480, height: 320),
  styleMask: [.titled, .closable, .miniaturizable, .resizable],
  backing: .buffered,
  defer: false
)
window.title = "AUV window management receiver"
window.makeKeyAndOrderFront(nil)

// Hand application focus back to the prior app, so `focus` has something to change.
DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
  previousApp?.activate(options: [])
  try! Data("ready".utf8).write(to: directory.appendingPathComponent("ready"))
}

// Bound the receiver even if the test terminates before its child cleanup.
DispatchQueue.main.asyncAfter(deadline: .now() + 60) { NSApp.terminate(nil) }

app.run()
