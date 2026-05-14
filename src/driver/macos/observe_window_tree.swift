import AppKit
import ApplicationServices
import Foundation

let appQuery = __APP_QUERY__.lowercased().trimmingCharacters(in: .whitespacesAndNewlines)
let maxDepth = __MAX_DEPTH__
let maxChildren = __MAX_CHILDREN__

func sanitize(_ raw: String?) -> String {
  guard let raw else { return "" }
  return raw
    .replacingOccurrences(of: "\t", with: " ")
    .replacingOccurrences(of: "\n", with: " ")
    .replacingOccurrences(of: "\r", with: " ")
    .trimmingCharacters(in: .whitespacesAndNewlines)
}

func matches(_ app: NSRunningApplication, query: String) -> Bool {
  let name = app.localizedName?.lowercased() ?? ""
  let bundleId = app.bundleIdentifier?.lowercased() ?? ""
  return name == query || name.contains(query) || bundleId == query || bundleId.contains(query)
}

func targetApp() -> NSRunningApplication? {
  let running = NSWorkspace.shared.runningApplications.filter { !$0.isTerminated }
  if appQuery.isEmpty {
    return NSWorkspace.shared.frontmostApplication ?? running.first
  }
  return running.first(where: { matches($0, query: appQuery) })
}

func attributeValue(_ element: AXUIElement, _ attribute: String) -> CFTypeRef? {
  var value: CFTypeRef?
  let result = AXUIElementCopyAttributeValue(element, attribute as CFString, &value)
  guard result == .success else { return nil }
  return value
}

func axValueAttribute(_ element: AXUIElement, _ attribute: String) -> AXValue? {
  guard let rawValue = attributeValue(element, attribute) else { return nil }
  guard CFGetTypeID(rawValue) == AXValueGetTypeID() else { return nil }
  return unsafeBitCast(rawValue, to: AXValue.self)
}

func axElementAttribute(_ element: AXUIElement, _ attribute: String) -> AXUIElement? {
  guard let rawValue = attributeValue(element, attribute) else { return nil }
  guard CFGetTypeID(rawValue) == AXUIElementGetTypeID() else { return nil }
  return unsafeBitCast(rawValue, to: AXUIElement.self)
}

func elementArrayAttribute(_ element: AXUIElement, _ attribute: String) -> [AXUIElement] {
  guard let rawValue = attributeValue(element, attribute) else { return [] }
  guard let array = rawValue as? NSArray else { return [] }
  return array.compactMap { item in
    let value = item as CFTypeRef
    guard CFGetTypeID(value) == AXUIElementGetTypeID() else { return nil }
    return unsafeBitCast(value, to: AXUIElement.self)
  }
}

func stringAttribute(_ element: AXUIElement, _ attribute: String) -> String {
  if let stringValue = attributeValue(element, attribute) as? String {
    return sanitize(stringValue)
  }
  if let numberValue = attributeValue(element, attribute) as? NSNumber {
    return numberValue.stringValue
  }
  return ""
}

func children(_ element: AXUIElement) -> [AXUIElement] {
  elementArrayAttribute(element, kAXChildrenAttribute as String)
}

func pointAttribute(_ element: AXUIElement, _ attribute: String) -> CGPoint? {
  guard let value = axValueAttribute(element, attribute) else { return nil }
  guard AXValueGetType(value) == .cgPoint else { return nil }
  var point = CGPoint.zero
  guard AXValueGetValue(value, .cgPoint, &point) else { return nil }
  return point
}

func sizeAttribute(_ element: AXUIElement, _ attribute: String) -> CGSize? {
  guard let value = axValueAttribute(element, attribute) else { return nil }
  guard AXValueGetType(value) == .cgSize else { return nil }
  var size = CGSize.zero
  guard AXValueGetValue(value, .cgSize, &size) else { return nil }
  return size
}

func bounds(_ element: AXUIElement) -> (Int, Int, Int, Int) {
  let point = pointAttribute(element, kAXPositionAttribute as String) ?? .zero
  let size = sizeAttribute(element, kAXSizeAttribute as String) ?? .zero
  return (
    Int(point.x.rounded()),
    Int(point.y.rounded()),
    Int(size.width.rounded()),
    Int(size.height.rounded())
  )
}

func firstWindow(_ appElement: AXUIElement) -> AXUIElement? {
  if let focused = axElementAttribute(appElement, kAXFocusedWindowAttribute as String) {
    return focused
  }
  return elementArrayAttribute(appElement, kAXWindowsAttribute as String).first
}

func printNode(_ element: AXUIElement, depth: Int, path: String) {
  let role = stringAttribute(element, kAXRoleAttribute as String)
  let subrole = stringAttribute(element, kAXSubroleAttribute as String)
  let title = stringAttribute(element, kAXTitleAttribute as String)
  let description = stringAttribute(element, kAXDescriptionAttribute as String)
  let help = stringAttribute(element, kAXHelpAttribute as String)
  let identifier = stringAttribute(element, kAXIdentifierAttribute as String)
  let placeholder = stringAttribute(element, "AXPlaceholderValue")
  let value = stringAttribute(element, kAXValueAttribute as String)
  let frame = bounds(element)

  print(
    "node\t\(depth)\t\(path)\t\(role)\t\(subrole)\t\(title)\t\(description)\t\(help)\t\(identifier)\t\(placeholder)\t\(value)\t\(frame.0)\t\(frame.1)\t\(frame.2)\t\(frame.3)"
  )
}

guard let app = targetApp() else {
  fputs("could not resolve target macOS app for AX tree observation\n", stderr)
  exit(1)
}

let appElement = AXUIElementCreateApplication(app.processIdentifier)
let rootElement = firstWindow(appElement) ?? appElement
let windowTitle = stringAttribute(rootElement, kAXTitleAttribute as String)

print("observedAt=\(ISO8601DateFormatter().string(from: Date()))")
print("appName=\(sanitize(app.localizedName))")
print("bundleId=\(sanitize(app.bundleIdentifier))")
print("pid=\(app.processIdentifier)")
print("windowTitle=\(windowTitle)")
print("rootRole=\(stringAttribute(rootElement, kAXRoleAttribute as String))")

var nodeCount = 0

func walk(_ element: AXUIElement, depth: Int, path: String) {
  nodeCount += 1
  printNode(element, depth: depth, path: path)
  if depth >= maxDepth {
    return
  }

  let visibleChildren = Array(children(element).prefix(maxChildren))
  for (index, child) in visibleChildren.enumerated() {
    walk(child, depth: depth + 1, path: "\(path).\(index)")
  }
}

walk(rootElement, depth: 0, path: "0")
print("nodeCount=\(nodeCount)")
