import AppKit
import Foundation

// Independent, opt-in AppKit receiver. File commands only reset the fixture;
// tested key events arrive through AUV and the operating system.
let directory = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
let title = CommandLine.arguments[2]

let app = NSApplication.shared
app.setActivationPolicy(.regular)

let menu = NSMenu()
let editItem = NSMenuItem(title: "Edit", action: nil, keyEquivalent: "")
let edit = NSMenu(title: "Edit")
edit.addItem(withTitle: "Select All", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
editItem.submenu = edit
menu.addItem(editItem)
app.mainMenu = menu

let window = NSWindow(
  contentRect: NSRect(x: 160, y: 160, width: 600, height: 320),
  styleMask: [.titled, .closable, .resizable],
  backing: .buffered,
  defer: false
)
window.title = title

let text = NSTextView(frame: NSRect(x: 0, y: 0, width: 600, height: 320))
text.isRichText = false
text.font = NSFont.monospacedSystemFont(ofSize: 22, weight: .regular)

window.contentView = text
window.makeKeyAndOrderFront(nil)
window.makeFirstResponder(text)
app.activate(ignoringOtherApps: true)

var caseId = "startup"
var events: [[String: Any]] = []

let log = directory.appendingPathComponent("native-state.jsonl")
FileManager.default.createFile(atPath: log.path, contents: nil)
let logHandle = try! FileHandle(forWritingTo: log)
var lastState = ""

func receipt() -> [String: Any] {
  let range = text.selectedRange()

  return [
    "caseId": caseId,
    "value": text.string,
    "selection": [range.location, range.location + range.length],
    "pid": ProcessInfo.processInfo.processIdentifier,
    "window": window.windowNumber,
    "active": app.isActive,
    "keyWindow": window.isKeyWindow,
    "mainWindow": window.isMainWindow,
    "firstResponder": window.firstResponder === text,
    "events": events,
  ]
}

func publish() {
  let value = receipt()
  try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
    .write(to: directory.appendingPathComponent("receipt.json"), options: .atomic)

  var state = value
  state.removeValue(forKey: "events")

  let data = try! JSONSerialization.data(withJSONObject: state, options: [.sortedKeys])
  let encoded = String(data: data, encoding: .utf8)!

  if encoded != lastState {
    state["timeMs"] = Date().timeIntervalSince1970 * 1000
    logHandle.write(try! JSONSerialization.data(withJSONObject: state, options: [.sortedKeys]))
    logHandle.write(Data([10]))
    lastState = encoded
  }
}

let monitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .keyUp, .flagsChanged]) { event in
  events.append([
    "timeMs": Date().timeIntervalSince1970 * 1000,
    "type": event.type.rawValue,
    "keyCode": event.keyCode,
    "characters": event.type == .flagsChanged ? "" : event.characters ?? "",
    "flags": event.modifierFlags.rawValue,
    "active": app.isActive,
    "keyWindow": window.isKeyWindow,
    "window": event.windowNumber,
  ])

  return event
}

Timer.scheduledTimer(withTimeInterval: 0.02, repeats: true) { _ in
  if let data = try? Data(contentsOf: directory.appendingPathComponent("command.json")),
     let command = try? JSONSerialization.jsonObject(with: data) as? [String: String],
     let next = command["caseId"], next != caseId
  {
    caseId = next
    text.string = command["initial"] ?? ""
    text.setSelectedRange(NSRange(location: (text.string as NSString).length, length: 0))
    window.makeFirstResponder(text)
    events = []
  }

  publish()
}

DispatchQueue.main.asyncAfter(deadline: .now() + 900) { app.terminate(nil) }

publish()
app.run()
