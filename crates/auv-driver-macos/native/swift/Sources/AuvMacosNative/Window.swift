import AppKit
import ApplicationServices
import CoreGraphics
import Darwin
import Foundation

private func boundsDict(_ value: NSDictionary?) -> [String: Int64]? {
  guard let value else { return nil }
  var rect = CGRect.zero
  guard CGRectMakeWithDictionaryRepresentation(value, &rect) else { return nil }
  return [
    "x": Int64(rect.origin.x.rounded()),
    "y": Int64(rect.origin.y.rounded()),
    "width": Int64(rect.size.width.rounded()),
    "height": Int64(rect.size.height.rounded())
  ]
}

func list_displays() -> NativeDisplayListResponse {
  let screens = NSScreen.screens
  let referenceHeight = NSScreen.main?.frame.height ?? screens.first?.frame.height ?? 0

  var ids: [Int64] = []
  var mainFlags: [Bool] = []
  var builtInFlags: [Bool] = []
  var boundsXValues: [Int64] = []
  var boundsYValues: [Int64] = []
  var boundsWidthValues: [Int64] = []
  var boundsHeightValues: [Int64] = []
  var visibleXValues: [Int64] = []
  var visibleYValues: [Int64] = []
  var visibleWidthValues: [Int64] = []
  var visibleHeightValues: [Int64] = []
  var scaleFactors: [Double] = []
  var pixelWidthValues: [Int64] = []
  var pixelHeightValues: [Int64] = []

  for screen in screens {
    let deviceDescription = screen.deviceDescription
    let displayId =
      (deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?.uint32Value ?? 0
    let frame = screen.frame
    let visibleFrame = screen.visibleFrame
    let scaleFactor = screen.backingScaleFactor

    ids.append(Int64(displayId))
    mainFlags.append(CGDisplayIsMain(displayId) != 0)
    builtInFlags.append(CGDisplayIsBuiltin(displayId) != 0)
    boundsXValues.append(Int64(frame.origin.x.rounded()))
    boundsYValues.append(Int64((referenceHeight - frame.origin.y - frame.height).rounded()))
    boundsWidthValues.append(Int64(frame.width.rounded()))
    boundsHeightValues.append(Int64(frame.height.rounded()))
    visibleXValues.append(Int64(visibleFrame.origin.x.rounded()))
    visibleYValues.append(Int64((referenceHeight - visibleFrame.origin.y - visibleFrame.height).rounded()))
    visibleWidthValues.append(Int64(visibleFrame.width.rounded()))
    visibleHeightValues.append(Int64(visibleFrame.height.rounded()))
    scaleFactors.append(Double(scaleFactor))
    pixelWidthValues.append(Int64((frame.width * scaleFactor).rounded()))
    pixelHeightValues.append(Int64((frame.height * scaleFactor).rounded()))
  }

  return NativeDisplayListResponse(
    captured_at: nativeNowIso8601(),
    ids: nativeVec(ids),
    main_flags: nativeVec(mainFlags),
    built_in_flags: nativeVec(builtInFlags),
    bounds_x_values: nativeVec(boundsXValues),
    bounds_y_values: nativeVec(boundsYValues),
    bounds_width_values: nativeVec(boundsWidthValues),
    bounds_height_values: nativeVec(boundsHeightValues),
    visible_x_values: nativeVec(visibleXValues),
    visible_y_values: nativeVec(visibleYValues),
    visible_width_values: nativeVec(visibleWidthValues),
    visible_height_values: nativeVec(visibleHeightValues),
    scale_factors: nativeVec(scaleFactors),
    pixel_width_values: nativeVec(pixelWidthValues),
    pixel_height_values: nativeVec(pixelHeightValues),
    error_message: nil,
    recovery_hint: nil
  )
}

func list_windows(request: NativeWindowListRequest) -> NativeWindowListResponse {
  let limit = max(Int(request.limit), 1)
  let appFilter = request.app_filter.toString().lowercased().trimmingCharacters(in: .whitespacesAndNewlines)
  let frontmostAppName = NSWorkspace.shared.frontmostApplication?.localizedName ?? ""
  let frontmostAppBundleId = NSWorkspace.shared.frontmostApplication?.bundleIdentifier ?? ""

  let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
  let rawWindowInfo = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] ?? []

  var appNames: [String] = []
  var ownerPids: [Int64] = []
  var ownerBundleIds: [String] = []
  var windowNumbers: [Int64] = []
  var layers: [Int64] = []
  var titles: [String] = []
  var xValues: [Int64] = []
  var yValues: [Int64] = []
  var widthValues: [Int64] = []
  var heightValues: [Int64] = []

  for window in rawWindowInfo {
    let ownerName = (window[kCGWindowOwnerName as String] as? String) ?? "Unknown"
    let ownerPid = window[kCGWindowOwnerPID as String] as? Int ?? 0
    let ownerBundleId = NSRunningApplication(processIdentifier: pid_t(ownerPid))?.bundleIdentifier ?? ""
    if !appFilter.isEmpty
      && !ownerName.lowercased().contains(appFilter)
      && ownerBundleId.lowercased() != appFilter {
      continue
    }

    let alpha = window[kCGWindowAlpha as String] as? Double ?? 1.0
    let layer = window[kCGWindowLayer as String] as? Int ?? 0
    let bounds = boundsDict(window[kCGWindowBounds as String] as? NSDictionary)
    let title =
      (window[kCGWindowName as String] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines)
      ?? ""
    let windowNumber = window[kCGWindowNumber as String] as? Int ?? 0

    if alpha <= 0 || (bounds?["width"] ?? 0) <= 1 || (bounds?["height"] ?? 0) <= 1 {
      continue
    }

    appNames.append(ownerName)
    ownerPids.append(Int64(ownerPid))
    ownerBundleIds.append(ownerBundleId)
    windowNumbers.append(Int64(windowNumber))
    layers.append(Int64(layer))
    titles.append(title)
    xValues.append(bounds?["x"] ?? 0)
    yValues.append(bounds?["y"] ?? 0)
    widthValues.append(bounds?["width"] ?? 0)
    heightValues.append(bounds?["height"] ?? 0)

    if appNames.count >= limit {
      break
    }
  }

  let frontmostWindowTitle: String
  if let bundleIndex = ownerBundleIds.firstIndex(of: frontmostAppBundleId) {
    frontmostWindowTitle = titles[bundleIndex]
  } else if let appIndex = appNames.firstIndex(of: frontmostAppName) {
    frontmostWindowTitle = titles[appIndex]
  } else {
    frontmostWindowTitle = ""
  }

  return NativeWindowListResponse(
    observed_at: nativeNowIso8601(),
    frontmost_app_name: frontmostAppName.intoRustString(),
    frontmost_app_bundle_id: frontmostAppBundleId.intoRustString(),
    frontmost_window_title: frontmostWindowTitle.intoRustString(),
    app_names: nativeStringVec(appNames),
    owner_pids: nativeVec(ownerPids),
    owner_bundle_ids: nativeStringVec(ownerBundleIds),
    window_numbers: nativeVec(windowNumbers),
    layers: nativeVec(layers),
    titles: nativeStringVec(titles),
    x_values: nativeVec(xValues),
    y_values: nativeVec(yValues),
    width_values: nativeVec(widthValues),
    height_values: nativeVec(heightValues),
    error_message: nil,
    recovery_hint: nil
  )
}

private struct NativeWindowFrame {
  let x: Int64
  let y: Int64
  let width: Int64
  let height: Int64
}

private typealias AXUIElementGetWindowFn = @convention(c) (
  AXUIElement,
  UnsafeMutablePointer<CGWindowID>
) -> AXError

// NOTICE: macOS does not expose a public AX attribute that reliably maps an
// `AXUIElement` window to the `kCGWindowNumber` captured by WindowServer.
// `_AXUIElementGetWindow` is a private ApplicationServices symbol used here at
// the native boundary so typed window mutation can target the resolved window
// id without falling back to ambiguous titles. Remove this once an
// owner-approved public mapping strategy replaces CGWindowID-targeted mutation.
private let axUIElementGetWindow: AXUIElementGetWindowFn? = {
  let symbolName = "_AXUIElementGetWindow"
  let globalHandle = UnsafeMutableRawPointer(bitPattern: -2)
  guard let symbol = dlsym(globalHandle, symbolName) else {
    return nil
  }
  return unsafeBitCast(symbol, to: AXUIElementGetWindowFn.self)
}()

private func windowAxAttributeValue(_ element: AXUIElement, _ attribute: String) -> CFTypeRef? {
  var value: CFTypeRef?
  let result = AXUIElementCopyAttributeValue(element, attribute as CFString, &value)
  guard result == .success else { return nil }
  return value
}

private func windowAxElementAttribute(_ element: AXUIElement, _ attribute: String) -> AXUIElement? {
  guard let rawValue = windowAxAttributeValue(element, attribute) else { return nil }
  guard CFGetTypeID(rawValue) == AXUIElementGetTypeID() else { return nil }
  return unsafeBitCast(rawValue, to: AXUIElement.self)
}

private func windowAxElementArrayAttribute(_ element: AXUIElement, _ attribute: String) -> [AXUIElement] {
  guard let rawValue = windowAxAttributeValue(element, attribute) else { return [] }
  guard let array = rawValue as? NSArray else { return [] }
  return array.compactMap { item in
    let value = item as CFTypeRef
    guard CFGetTypeID(value) == AXUIElementGetTypeID() else { return nil }
    return unsafeBitCast(value, to: AXUIElement.self)
  }
}

private func windowAxStringAttribute(_ element: AXUIElement, _ attribute: String) -> String {
  if let stringValue = windowAxAttributeValue(element, attribute) as? String {
    return nativeSanitize(stringValue)
  }
  if let numberValue = windowAxAttributeValue(element, attribute) as? NSNumber {
    return numberValue.stringValue
  }
  return ""
}

private func windowAxIntAttribute(_ element: AXUIElement, _ attribute: String) -> Int64? {
  if let numberValue = windowAxAttributeValue(element, attribute) as? NSNumber {
    return numberValue.int64Value
  }
  return nil
}

private func windowAxCgWindowId(_ element: AXUIElement) -> Int64? {
  guard let axUIElementGetWindow else { return nil }
  var windowId = CGWindowID(0)
  let result = axUIElementGetWindow(element, &windowId)
  guard result == .success, windowId > 0 else { return nil }
  return Int64(windowId)
}

private func windowAxBoolAttribute(_ element: AXUIElement, _ attribute: String) -> Bool {
  guard let value = windowAxAttributeValue(element, attribute) else { return false }
  if CFGetTypeID(value) == CFBooleanGetTypeID() {
    return CFBooleanGetValue((value as! CFBoolean))
  }
  if let number = value as? NSNumber {
    return number.boolValue
  }
  return false
}

private func windowAxValueAttribute(_ element: AXUIElement, _ attribute: String) -> AXValue? {
  guard let rawValue = windowAxAttributeValue(element, attribute) else { return nil }
  guard CFGetTypeID(rawValue) == AXValueGetTypeID() else { return nil }
  return unsafeBitCast(rawValue, to: AXValue.self)
}

private func windowAxPointAttribute(_ element: AXUIElement, _ attribute: String) -> CGPoint? {
  guard let value = windowAxValueAttribute(element, attribute) else { return nil }
  guard AXValueGetType(value) == .cgPoint else { return nil }
  var point = CGPoint.zero
  guard AXValueGetValue(value, .cgPoint, &point) else { return nil }
  return point
}

private func windowAxSizeAttribute(_ element: AXUIElement, _ attribute: String) -> CGSize? {
  guard let value = windowAxValueAttribute(element, attribute) else { return nil }
  guard AXValueGetType(value) == .cgSize else { return nil }
  var size = CGSize.zero
  guard AXValueGetValue(value, .cgSize, &size) else { return nil }
  return size
}

private func windowAxFrame(_ element: AXUIElement) -> NativeWindowFrame {
  let point = windowAxPointAttribute(element, kAXPositionAttribute as String) ?? .zero
  let size = windowAxSizeAttribute(element, kAXSizeAttribute as String) ?? .zero
  return NativeWindowFrame(
    x: Int64(point.x.rounded()),
    y: Int64(point.y.rounded()),
    width: Int64(size.width.rounded()),
    height: Int64(size.height.rounded())
  )
}

private func emptyWindowMutationResponse(
  message: String,
  recovery: String,
  before: NativeWindowFrame = NativeWindowFrame(x: 0, y: 0, width: 0, height: 0),
  after: NativeWindowFrame = NativeWindowFrame(x: 0, y: 0, width: 0, height: 0),
  wasMinimized: Bool = false,
  isMinimized: Bool = false
) -> NativeWindowMutationResponse {
  NativeWindowMutationResponse(
    performed_action: "".intoRustString(),
    path: "".intoRustString(),
    before_x: before.x,
    before_y: before.y,
    before_width: before.width,
    before_height: before.height,
    after_x: after.x,
    after_y: after.y,
    after_width: after.width,
    after_height: after.height,
    was_minimized: wasMinimized,
    is_minimized: isMinimized,
    error_message: message.intoRustString(),
    recovery_hint: recovery.intoRustString()
  )
}

private func resolveWindowMutationTarget(
  request: NativeWindowMutationRequest
) -> (window: AXUIElement, path: String)? {
  let appElement = AXUIElementCreateApplication(pid_t(request.pid))
  let windows = windowAxElementArrayAttribute(appElement, kAXWindowsAttribute as String)
  let requestedWindowNumber = request.window_number
  if requestedWindowNumber > 0 {
    for (index, window) in windows.enumerated() {
      if windowAxCgWindowId(window) == requestedWindowNumber {
        return (window, "pid=\(request.pid) window_number=\(requestedWindowNumber) ax_window_index=\(index)")
      }
    }
    return nil
  }

  let requestedTitle = request.title.toString().trimmingCharacters(in: .whitespacesAndNewlines)
  if !requestedTitle.isEmpty {
    for (index, window) in windows.enumerated() {
      if windowAxStringAttribute(window, kAXTitleAttribute as String) == requestedTitle {
        return (window, "pid=\(request.pid) title=\(requestedTitle) ax_window_index=\(index)")
      }
    }
  }

  if requestedWindowNumber <= 0 && requestedTitle.isEmpty,
     let focusedWindow = windowAxElementAttribute(appElement, kAXFocusedWindowAttribute as String) {
    return (focusedWindow, "pid=\(request.pid) focused_window")
  }

  return nil
}

private func setWindowPosition(_ window: AXUIElement, x: Int64, y: Int64) -> AXError {
  var point = CGPoint(x: CGFloat(x), y: CGFloat(y))
  guard let value = AXValueCreate(.cgPoint, &point) else { return .failure }
  return AXUIElementSetAttributeValue(window, kAXPositionAttribute as CFString, value)
}

private func setWindowSize(_ window: AXUIElement, width: Int64, height: Int64) -> AXError {
  var size = CGSize(width: CGFloat(width), height: CGFloat(height))
  guard let value = AXValueCreate(.cgSize, &size) else { return .failure }
  return AXUIElementSetAttributeValue(window, kAXSizeAttribute as CFString, value)
}

func mutate_window(request: NativeWindowMutationRequest) -> NativeWindowMutationResponse {
  // TODO(window-management-api-task3): pointer and foreground fallback are
  // deferred because this task exposes only the native AX bridge; add fallback
  // policy when WindowApi dispatch is introduced.
  guard request.pid >= 0 && request.pid <= Int64(Int32.max),
        NSRunningApplication(processIdentifier: pid_t(request.pid)) != nil else {
    return emptyWindowMutationResponse(
      message: "could not resolve target AX app for pid \(request.pid)",
      recovery: "verify the app is still running, refresh the window list, and retry with a current pid"
    )
  }

  guard let target = resolveWindowMutationTarget(request: request) else {
    return emptyWindowMutationResponse(
      message: "could not resolve target AX window for pid \(request.pid), window_number \(request.window_number), title \(request.title.toString())",
      recovery: "verify Accessibility permission, refresh the window list, and retry with pid plus window_number or title"
    )
  }

  let window = target.window
  let before = windowAxFrame(window)
  let wasMinimized = windowAxBoolAttribute(window, kAXMinimizedAttribute as String)
  var action = ""

  switch request.kind {
  case .MoveTo:
    let result = setWindowPosition(window, x: request.x, y: request.y)
    guard result == .success else {
      return emptyWindowMutationResponse(
        message: "AXUIElementSetAttributeValue(kAXPositionAttribute) returned \(result.rawValue)",
        recovery: "verify the target window accepts AXPosition mutations and retry",
        before: before,
        after: windowAxFrame(window),
        wasMinimized: wasMinimized,
        isMinimized: windowAxBoolAttribute(window, kAXMinimizedAttribute as String)
      )
    }
    action = "move_to"
  case .Resize:
    let result = setWindowSize(window, width: request.width, height: request.height)
    guard result == .success else {
      return emptyWindowMutationResponse(
        message: "AXUIElementSetAttributeValue(kAXSizeAttribute) returned \(result.rawValue)",
        recovery: "verify the target window accepts AXSize mutations and retry",
        before: before,
        after: windowAxFrame(window),
        wasMinimized: wasMinimized,
        isMinimized: windowAxBoolAttribute(window, kAXMinimizedAttribute as String)
      )
    }
    action = "resize"
  case .SetFrame:
    // TODO(window-set-frame-transaction): rollback is deferred because this
    // slice only exposes native AX mutation and verification; add transactional
    // recovery when owner approves rollback policy for partially moved windows.
    let positionResult = setWindowPosition(window, x: request.x, y: request.y)
    guard positionResult == .success else {
      return emptyWindowMutationResponse(
        message: "AXUIElementSetAttributeValue(kAXPositionAttribute) returned \(positionResult.rawValue)",
        recovery: "verify the target window accepts AXPosition mutations and retry",
        before: before,
        after: windowAxFrame(window),
        wasMinimized: wasMinimized,
        isMinimized: windowAxBoolAttribute(window, kAXMinimizedAttribute as String)
      )
    }
    let sizeResult = setWindowSize(window, width: request.width, height: request.height)
    guard sizeResult == .success else {
      return emptyWindowMutationResponse(
        message: "AXUIElementSetAttributeValue(kAXSizeAttribute) returned \(sizeResult.rawValue)",
        recovery: "verify the target window accepts AXSize mutations and retry",
        before: before,
        after: windowAxFrame(window),
        wasMinimized: wasMinimized,
        isMinimized: windowAxBoolAttribute(window, kAXMinimizedAttribute as String)
      )
    }
    action = "set_frame"
  case .Minimize:
    let result = AXUIElementSetAttributeValue(window, kAXMinimizedAttribute as CFString, kCFBooleanTrue)
    guard result == .success else {
      return emptyWindowMutationResponse(
        message: "AXUIElementSetAttributeValue(kAXMinimizedAttribute=true) returned \(result.rawValue)",
        recovery: "verify the target window accepts AXMinimized mutations and retry",
        before: before,
        after: windowAxFrame(window),
        wasMinimized: wasMinimized,
        isMinimized: windowAxBoolAttribute(window, kAXMinimizedAttribute as String)
      )
    }
    action = "minimize"
  case .Restore:
    let result = AXUIElementSetAttributeValue(window, kAXMinimizedAttribute as CFString, kCFBooleanFalse)
    guard result == .success else {
      return emptyWindowMutationResponse(
        message: "AXUIElementSetAttributeValue(kAXMinimizedAttribute=false) returned \(result.rawValue)",
        recovery: "verify the target window accepts AXMinimized mutations and retry",
        before: before,
        after: windowAxFrame(window),
        wasMinimized: wasMinimized,
        isMinimized: windowAxBoolAttribute(window, kAXMinimizedAttribute as String)
      )
    }
    action = "restore"
  case .Zoom:
    guard let zoomButton = windowAxElementAttribute(window, kAXZoomButtonAttribute as String) else {
      return emptyWindowMutationResponse(
        message: "target AX window does not expose kAXZoomButtonAttribute",
        recovery: "choose a window that exposes a zoom button or use move/resize/set_frame instead",
        before: before,
        after: before,
        wasMinimized: wasMinimized,
        isMinimized: wasMinimized
      )
    }
    let result = AXUIElementPerformAction(zoomButton, kAXPressAction as CFString)
    guard result == .success else {
      return emptyWindowMutationResponse(
        message: "AXUIElementPerformAction(kAXPressAction) on kAXZoomButtonAttribute returned \(result.rawValue)",
        recovery: "verify the target window zoom button accepts AXPress and retry",
        before: before,
        after: windowAxFrame(window),
        wasMinimized: wasMinimized,
        isMinimized: windowAxBoolAttribute(window, kAXMinimizedAttribute as String)
      )
    }
    action = "zoom"
  }

  let after = windowAxFrame(window)
  let isMinimized = windowAxBoolAttribute(window, kAXMinimizedAttribute as String)
  return NativeWindowMutationResponse(
    performed_action: action.intoRustString(),
    path: target.path.intoRustString(),
    before_x: before.x,
    before_y: before.y,
    before_width: before.width,
    before_height: before.height,
    after_x: after.x,
    after_y: after.y,
    after_width: after.width,
    after_height: after.height,
    was_minimized: wasMinimized,
    is_minimized: isMinimized,
    error_message: nil,
    recovery_hint: nil
  )
}

func bundle_ids_by_pid(request: NativeBundleIdsByPidRequest) -> NativeBundleIdsByPidResponse {
  var resolvedPids: [Int64] = []
  var bundleIds: [String] = []

  for requestedPid in request.pids {
    guard requestedPid >= 0 && requestedPid <= Int64(Int32.max) else {
      continue
    }
    if let app = NSRunningApplication(processIdentifier: pid_t(requestedPid)),
       let bundleId = app.bundleIdentifier,
       !bundleId.isEmpty {
      resolvedPids.append(requestedPid)
      bundleIds.append(bundleId)
    }
  }

  return NativeBundleIdsByPidResponse(
    pids: nativeVec(resolvedPids),
    bundle_ids: nativeStringVec(bundleIds),
    error_message: nil,
    recovery_hint: nil
  )
}
