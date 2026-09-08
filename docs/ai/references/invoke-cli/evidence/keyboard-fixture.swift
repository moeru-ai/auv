// Manual AppKit validation fixture. Run inside a temporary .app with bundle ID
// ai.moeru.auv.keyboard-validation-fixture; pass a writable JSON output path.
// The output measures control effects independently from AUV event submission.
import AppKit

final class Fixture: NSObject, NSApplicationDelegate, NSTextFieldDelegate {
  var window: NSWindow!
  var field: NSTextField!
  var submissions = 0
  let output = URL(fileURLWithPath: CommandLine.arguments[1])
  func applicationDidFinishLaunching(_ notification: Notification) {
    window = NSWindow(contentRect: NSRect(x: 100, y: 150, width: 620, height: 160), styleMask: [.titled, .closable], backing: .buffered, defer: false)
    window.title = "AUV keyboard validation"
    field = NSTextField(frame: NSRect(x: 25, y: 60, width: 570, height: 35))
    field.stringValue = "0123456789"
    field.delegate = self
    field.target = self
    field.action = #selector(submit)
    window.contentView!.addSubview(field)
    window.makeKeyAndOrderFront(nil)
    NSApp.activate(ignoringOtherApps: true)
    window.makeFirstResponder(field)
    if let editor = field.currentEditor() { editor.selectedRange = NSRange(location: 10, length: 0) }
    record("ready")
  }
  func controlTextDidChange(_ notification: Notification) { record("change") }
  @objc func submit() { submissions += 1; record("submit") }
  func record(_ event: String) {
    let data = try! JSONSerialization.data(withJSONObject: ["event": event, "text": field.stringValue, "submissions": submissions], options: [.sortedKeys])
    try! data.write(to: output, options: [.atomic])
  }
}
let app = NSApplication.shared
app.setActivationPolicy(.regular)
let fixture = Fixture()
app.delegate = fixture
app.run()
