import AppKit
import CoreGraphics
import Foundation

func boundsDict(_ value: NSDictionary?) -> [String: Int]? {
  guard let value else { return nil }
  var rect = CGRect.zero
  guard CGRectMakeWithDictionaryRepresentation(value, &rect) else { return nil }
  return [
    "x": Int(rect.origin.x.rounded()),
    "y": Int(rect.origin.y.rounded()),
    "width": Int(rect.size.width.rounded()),
    "height": Int(rect.size.height.rounded())
  ]
}

let limit = __LIMIT__
let appFilter = __APP_FILTER__.lowercased().trimmingCharacters(in: .whitespacesAndNewlines)
let frontmostAppName = NSWorkspace.shared.frontmostApplication?.localizedName ?? ""

let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
let rawWindowInfo = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] ?? []
var windows: [[String: Any]] = []
for window in rawWindowInfo {
  let ownerName = (window[kCGWindowOwnerName as String] as? String) ?? "Unknown"
  if !appFilter.isEmpty && !ownerName.lowercased().contains(appFilter) {
    continue
  }

  let alpha = window[kCGWindowAlpha as String] as? Double ?? 1.0
  let layer = window[kCGWindowLayer as String] as? Int ?? 0
  let bounds = boundsDict(window[kCGWindowBounds as String] as? NSDictionary)
  let title = (window[kCGWindowName as String] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
  let ownerPid = window[kCGWindowOwnerPID as String] as? Int ?? 0

  if alpha <= 0 || (bounds?["width"] ?? 0) <= 1 || (bounds?["height"] ?? 0) <= 1 {
    continue
  }

  windows.append([
    "appName": ownerName,
    "title": title,
    "ownerPid": ownerPid,
    "layer": layer,
    "x": bounds?["x"] ?? 0,
    "y": bounds?["y"] ?? 0,
    "width": bounds?["width"] ?? 0,
    "height": bounds?["height"] ?? 0
  ])

  if windows.count >= limit {
    break
  }
}

let frontmostWindowTitle = windows.first(where: { ($0["appName"] as? String) == frontmostAppName })?["title"] as? String ?? ""

print("frontmostAppName=\(frontmostAppName)")
print("frontmostWindowTitle=\(frontmostWindowTitle)")
print("observedAt=\(ISO8601DateFormatter().string(from: Date()))")
print("windowCount=\(windows.count)")
for window in windows {
  let appName = window["appName"] as? String ?? "Unknown"
  let title = window["title"] as? String ?? ""
  let ownerPid = window["ownerPid"] as? Int ?? 0
  let layer = window["layer"] as? Int ?? 0
  let x = window["x"] as? Int ?? 0
  let y = window["y"] as? Int ?? 0
  let width = window["width"] as? Int ?? 0
  let height = window["height"] as? Int ?? 0
  print("window\t\(appName)\t\(ownerPid)\t\(layer)\t\(title)\t\(x)\t\(y)\t\(width)\t\(height)")
}
