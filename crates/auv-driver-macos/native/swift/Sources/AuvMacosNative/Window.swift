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
  // Frames stay in Cocoa coordinates (bottom-left origin, y up) in
  // `NSScreen.screens` order; the first screen is the primary display.
  // `decode_display_response` converts them to global CG coordinates.
  let screens = NSScreen.screens

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
    boundsYValues.append(Int64(frame.origin.y.rounded()))
    boundsWidthValues.append(Int64(frame.width.rounded()))
    boundsHeightValues.append(Int64(frame.height.rounded()))
    visibleXValues.append(Int64(visibleFrame.origin.x.rounded()))
    visibleYValues.append(Int64(visibleFrame.origin.y.rounded()))
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

/// AX facts for one window, keyed by its CGWindowID.
private struct WindowAxState {
  let minimized: Bool
  let fullscreen: Bool
}

// NOTICE(window-list-ax-timeout): the default AX messaging timeout is about
// 6 s per call, so one hung application would stall every listing. Reads in
// the listing give up after this bound and leave that app's state unknown.
private let windowListAxTimeout: Float = 0.5

/// AX window state for every window of `pid`. Empty when Accessibility is not
/// trusted or the application does not answer.
private func windowAxStates(pid: pid_t) -> [Int64: WindowAxState] {
  let appElement = AXUIElementCreateApplication(pid)
  AXUIElementSetMessagingTimeout(appElement, windowListAxTimeout)
  var states: [Int64: WindowAxState] = [:]
  for window in windowAxElementArrayAttribute(appElement, kAXWindowsAttribute as String) {
    guard let windowId = windowAxCgWindowId(window) else { continue }
    states[windowId] = WindowAxState(
      minimized: windowAxBoolAttribute(window, kAXMinimizedAttribute as String),
      // NOTICE: `AXFullScreen` has no public constant in ApplicationServices;
      // AppKit windows publish it under this name.
      fullscreen: windowAxBoolAttribute(window, "AXFullScreen")
    )
  }
  return states
}

/// The CGWindowID of the focused window of the frontmost application, or nil
/// when it has none. `known` is false when focus could not be read.
private func frontmostFocusedWindow(_ frontmost: NSRunningApplication?) -> (known: Bool, windowId: Int64?) {
  guard AXIsProcessTrusted(), let frontmost else { return (false, nil) }
  let appElement = AXUIElementCreateApplication(frontmost.processIdentifier)
  AXUIElementSetMessagingTimeout(appElement, windowListAxTimeout)
  guard let focused = windowAxElementAttribute(appElement, kAXFocusedWindowAttribute as String) else {
    return (true, nil)
  }
  return (true, windowAxCgWindowId(focused))
}

/// The application that is frontmost now.
// NOTICE(macos-frontmost-stale): `NSWorkspace.frontmostApplication` is updated
// from workspace notifications on the main run loop, which the Runner and CLI
// never pump, so a process keeps reporting the app that was frontmost at its
// first read (reproduced 2026-11-03: it still named the terminal 3 s after
// `open -a TextEdit`, and changed only after `RunLoop.run(until:)`). That made
// `window.focus` fail verification right after a successful activation. The
// system-wide AX element answers live; NSWorkspace stays the fallback when AX
// is not trusted or has no focused application.
private func currentFrontmostApplication() -> NSRunningApplication? {
  if AXIsProcessTrusted() {
    let systemWide = AXUIElementCreateSystemWide()
    AXUIElementSetMessagingTimeout(systemWide, windowListAxTimeout)
    var value: CFTypeRef?
    if AXUIElementCopyAttributeValue(systemWide, kAXFocusedApplicationAttribute as CFString, &value) == .success,
       let value, CFGetTypeID(value) == AXUIElementGetTypeID() {
      var pid: pid_t = 0
      if AXUIElementGetPid(value as! AXUIElement, &pid) == .success,
         let app = NSRunningApplication(processIdentifier: pid) {
        return app
      }
    }
  }
  return NSWorkspace.shared.frontmostApplication
}

func list_windows(request: NativeWindowListRequest) -> NativeWindowListResponse {
  let limit = max(Int(request.limit), 1)
  let appFilter = request.app_filter.toString().lowercased().trimmingCharacters(in: .whitespacesAndNewlines)
  let includeOffscreen = request.include_offscreen
  let frontmostApplication = currentFrontmostApplication()
  let frontmostAppName = frontmostApplication?.localizedName ?? ""
  let frontmostAppBundleId = frontmostApplication?.bundleIdentifier ?? ""

  // `.optionAll` also returns windows that are not on screen: minimized
  // windows, windows on other Spaces, tabs, and many invisible helper windows.
  let options: CGWindowListOption =
    includeOffscreen ? [.optionAll, .excludeDesktopElements] : [.optionOnScreenOnly, .excludeDesktopElements]
  let rawWindowInfo = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] ?? []

  var applications: [Int: NSRunningApplication?] = [:]
  func application(_ pid: Int) -> NSRunningApplication? {
    if let cached = applications[pid] { return cached }
    let app = NSRunningApplication(processIdentifier: pid_t(pid))
    applications[pid] = app
    return app
  }
  var axStates: [Int: [Int64: WindowAxState]] = [:]
  func axState(_ pid: Int, _ windowNumber: Int64) -> WindowAxState? {
    if axStates[pid] == nil {
      // NOTICE(window-list-ax-latency): the first AX message to a process costs
      // about 10-40 ms. Only regular (Dock) applications own user windows, so
      // agents and XPC services are not asked.
      // An unresolved application is asked anyway, because
      // `NSRunningApplication(processIdentifier:)` can transiently miss a
      // live app on worker threads (see `inputProcessIsRunning`).
      let isRegular = application(pid).map { $0.activationPolicy == .regular } ?? true
      axStates[pid] = isRegular ? windowAxStates(pid: pid_t(pid)) : [:]
    }
    return axStates[pid]?[windowNumber]
  }
  func matchesAppFilter(name: String, bundleId: String) -> Bool {
    appFilter.isEmpty || name.lowercased().contains(appFilter) || bundleId.lowercased() == appFilter
  }
  // Only the frontmost application has a focused window. A listing scoped to
  // another application skips the AX message to the frontmost one, which costs
  // 10-40 ms when it is the first to that process; no window it returns can be
  // focused, so focus stays known.
  let focus: (known: Bool, windowId: Int64?)
  if !includeOffscreen {
    focus = (known: false, windowId: nil)
  } else if matchesAppFilter(name: frontmostAppName, bundleId: frontmostAppBundleId) {
    focus = frontmostFocusedWindow(frontmostApplication)
  } else {
    focus = (known: AXIsProcessTrusted(), windowId: nil)
  }

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
  var onScreenFlags: [Bool] = []
  var axStateFlags: [Bool] = []
  var minimizedFlags: [Bool] = []
  var fullscreenFlags: [Bool] = []
  var focusedFlags: [Bool] = []

  for window in rawWindowInfo {
    let alpha = window[kCGWindowAlpha as String] as? Double ?? 1.0
    let bounds = boundsDict(window[kCGWindowBounds as String] as? NSDictionary)
    // Dropped before the owner lookup: `.optionAll` returns hundreds of
    // transparent or degenerate helper windows, and each new owner costs an
    // `NSRunningApplication` lookup.
    if alpha <= 0 || (bounds?["width"] ?? 0) <= 1 || (bounds?["height"] ?? 0) <= 1 {
      continue
    }
    let ownerName = (window[kCGWindowOwnerName as String] as? String) ?? "Unknown"
    let ownerPid = window[kCGWindowOwnerPID as String] as? Int ?? 0
    let ownerBundleId = application(ownerPid)?.bundleIdentifier ?? ""
    if !matchesAppFilter(name: ownerName, bundleId: ownerBundleId) {
      continue
    }

    let layer = window[kCGWindowLayer as String] as? Int ?? 0
    let title =
      (window[kCGWindowName as String] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines)
      ?? ""
    let windowNumber = Int64(window[kCGWindowNumber as String] as? Int ?? 0)
    let onScreen = !includeOffscreen || (window[kCGWindowIsOnscreen as String] as? Bool ?? false)

    let ax = includeOffscreen && layer == 0 ? axState(ownerPid, windowNumber) : nil
    // An off-screen window is listed only when its application reports it as
    // an AX window (for example a minimized window). This drops the helper
    // and tab windows that `.optionAll` returns.
    // TODO(macos-window-space): windows on other Spaces are not AX windows of
    // their application and stay unlisted; listing them needs private CGS API.
    if !onScreen && ax == nil {
      continue
    }

    appNames.append(ownerName)
    ownerPids.append(Int64(ownerPid))
    ownerBundleIds.append(ownerBundleId)
    windowNumbers.append(windowNumber)
    layers.append(Int64(layer))
    titles.append(title)
    xValues.append(bounds?["x"] ?? 0)
    yValues.append(bounds?["y"] ?? 0)
    widthValues.append(bounds?["width"] ?? 0)
    heightValues.append(bounds?["height"] ?? 0)
    onScreenFlags.append(onScreen)
    axStateFlags.append(ax != nil)
    minimizedFlags.append(ax?.minimized ?? false)
    fullscreenFlags.append(ax?.fullscreen ?? false)
    focusedFlags.append(focus.windowId == windowNumber)

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
    on_screen_flags: nativeVec(onScreenFlags),
    ax_state_flags: nativeVec(axStateFlags),
    minimized_flags: nativeVec(minimizedFlags),
    fullscreen_flags: nativeVec(fullscreenFlags),
    focused_flags: nativeVec(focusedFlags),
    focus_known: focus.known,
    error_message: nil,
    recovery_hint: nil
  )
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

private func windowAxSizeAttribute(_ element: AXUIElement, _ attribute: String) -> CGSize? {
  guard let value = windowAxValueAttribute(element, attribute) else { return nil }
  guard AXValueGetType(value) == .cgSize else { return nil }
  var size = CGSize.zero
  guard AXValueGetValue(value, .cgSize, &size) else { return nil }
  return size
}
// NOTICE(window-mutation-ax-timeout): a hung application must not stall a
// window mutation for the default ~6 s per AX message. trycua/cua
// `libs/cua-driver/rust/crates/platform-macos/src/tools/set_window_frame.rs`
// uses the same bound (checked 2026-11-03).
private let windowMutationAxTimeout: Float = 2.0

private func windowMutationError(_ message: String, _ recovery: String) -> NativeWindowMutationResponse {
  NativeWindowMutationResponse(
    performed_action: "".intoRustString(),
    path: "".intoRustString(),
    error_message: message.intoRustString(),
    recovery_hint: recovery.intoRustString()
  )
}

/// The AX element of window `windowNumber` owned by `pid`, with the mutation
/// messaging timeout set on the application and window elements.
private func resolveMutationWindow(
  pid: Int64,
  windowNumber: Int64,
  title: String
) -> (window: AXUIElement, path: String)? {
  guard pid > 0 && pid <= Int64(Int32.max) else { return nil }
  let appElement = AXUIElementCreateApplication(pid_t(pid))
  AXUIElementSetMessagingTimeout(appElement, windowMutationAxTimeout)
  let windows = windowAxElementArrayAttribute(appElement, kAXWindowsAttribute as String)
  var resolved: (AXUIElement, String)?
  if windowNumber > 0 {
    if let index = windows.firstIndex(where: { windowAxCgWindowId($0) == windowNumber }) {
      resolved = (windows[index], "pid=\(pid) window_number=\(windowNumber) ax_window_index=\(index)")
    }
  } else if !title.isEmpty {
    if let index = windows.firstIndex(where: { windowAxStringAttribute($0, kAXTitleAttribute as String) == title }) {
      resolved = (windows[index], "pid=\(pid) title=\(title) ax_window_index=\(index)")
    }
  } else if let focusedWindow = windowAxElementAttribute(appElement, kAXFocusedWindowAttribute as String) {
    resolved = (focusedWindow, "pid=\(pid) focused_window")
  }
  guard let resolved else { return nil }
  AXUIElementSetMessagingTimeout(resolved.0, windowMutationAxTimeout)
  return (resolved.0, resolved.1)
}

private func setWindowPosition(_ window: AXUIElement, x: Double, y: Double) -> AXError {
  var point = CGPoint(x: CGFloat(x), y: CGFloat(y))
  guard let value = AXValueCreate(.cgPoint, &point) else { return .failure }
  return AXUIElementSetAttributeValue(window, kAXPositionAttribute as CFString, value)
}

private func setWindowSize(_ window: AXUIElement, width: Double, height: Double) -> AXError {
  var size = CGSize(width: CGFloat(width), height: CGFloat(height))
  guard let value = AXValueCreate(.cgSize, &size) else { return .failure }
  return AXUIElementSetAttributeValue(window, kAXSizeAttribute as CFString, value)
}

/// One geometry pass. Rust owns the corrective-pass policy and confirms each
/// pass against `window_server_frame`, not against the AX values written here.
func write_window_frame(request: NativeWindowFrameWriteRequest) -> NativeActionResponse {
  guard let target = resolveMutationWindow(pid: request.pid, windowNumber: request.window_number, title: "") else {
    return nativeActionError(
      "could not resolve target AX window for pid \(request.pid), window_number \(request.window_number)",
      "verify Accessibility permission, refresh the window list, and retry with a current window"
    )
  }
  // NOTICE(macos-ax-position-before-size): on macOS Tahoe, writing AXPosition
  // right after AXSize can silently restore the old size while both writes
  // report success. Position is written first. See trycua/cua
  // `platform-macos/src/tools/set_window_frame.rs` (checked 2026-11-03).
  if request.write_position {
    let result = setWindowPosition(target.window, x: request.x, y: request.y)
    guard result == .success else {
      return nativeActionError(
        "AXUIElementSetAttributeValue(kAXPositionAttribute) returned \(result.rawValue) (\(target.path))",
        "verify the target window accepts AXPosition and retry"
      )
    }
  }
  if request.write_size {
    let result = setWindowSize(target.window, width: request.width, height: request.height)
    guard result == .success else {
      return nativeActionError(
        "AXUIElementSetAttributeValue(kAXSizeAttribute) returned \(result.rawValue) (\(target.path))",
        "the window may not be resizable; retry with only an origin"
      )
    }
  }
  return nativeActionOk()
}

func window_server_frame(window_number: Int64) -> NativeWindowServerFrameResponse {
  let missing = NativeWindowServerFrameResponse(found: false, x: 0, y: 0, width: 0, height: 0)
  guard window_number > 0, window_number <= Int64(UInt32.max),
        let windows = CGWindowListCopyWindowInfo(.optionIncludingWindow, CGWindowID(window_number)) as? [[String: Any]],
        let window = windows.first(where: { ($0[kCGWindowNumber as String] as? NSNumber)?.int64Value == window_number }),
        let bounds = window[kCGWindowBounds as String] as? NSDictionary
  else { return missing }
  var rect = CGRect.zero
  guard CGRectMakeWithDictionaryRepresentation(bounds, &rect) else { return missing }
  return NativeWindowServerFrameResponse(
    found: true,
    x: Double(rect.origin.x),
    y: Double(rect.origin.y),
    width: Double(rect.size.width),
    height: Double(rect.size.height)
  )
}

/// Minimize, unminimize or raise one window. The caller reads the state
/// back through the window listing after its settle delay.
func mutate_window(request: NativeWindowMutationRequest) -> NativeWindowMutationResponse {
  guard let target = resolveMutationWindow(
    pid: request.pid,
    windowNumber: request.window_number,
    title: request.title.toString().trimmingCharacters(in: .whitespacesAndNewlines)
  ) else {
    return windowMutationError(
      "could not resolve target AX window for pid \(request.pid), window_number \(request.window_number), title \(request.title.toString())",
      "verify Accessibility permission, refresh the window list, and retry with pid plus window_number or title"
    )
  }

  let window = target.window
  let action: String
  let result: AXError
  switch request.kind {
  case .Minimize:
    action = "set AXMinimized=true"
    result = AXUIElementSetAttributeValue(window, kAXMinimizedAttribute as CFString, kCFBooleanTrue)
  case .Unminimize:
    action = "set AXMinimized=false"
    result = AXUIElementSetAttributeValue(window, kAXMinimizedAttribute as CFString, kCFBooleanFalse)
  case .Raise:
    action = "kAXRaiseAction"
    result = AXUIElementPerformAction(window, kAXRaiseAction as CFString)
  }
  guard result == .success else {
    return windowMutationError("\(action) returned AXError \(result.rawValue)", "verify the target window supports \(action) and retry")
  }
  return NativeWindowMutationResponse(
    performed_action: action.intoRustString(),
    path: target.path.intoRustString(),
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

// NOTICE: On Runner worker threads, NSRunningApplication(pid:) can transiently
// return nil for a live app, and NSWorkspace's cached list can miss a restarted
// app (PR #177 live regressions). Process liveness must not depend on AppKit's
// main-RunLoop cache: `https://developer.apple.com/documentation/appkit/nsrunningapplication`.
private func inputProcessIsRunning(_ pid: Int64) -> Bool {
  guard pid > 0, pid <= Int64(Int32.max) else { return false }
  return kill(pid_t(pid), 0) == 0 || errno == EPERM
}

// Retain observed application identities across transient LaunchServices misses.
// Entries never authorize input by themselves: liveness and the executable path
// are checked against the kernel on every lookup. Dead PIDs are removed eagerly.
private let inputApplicationsLock = NSLock()
private var inputApplications: [pid_t: NSRunningApplication] = [:]

private func inputApplicationMatchesProcess(_ app: NSRunningApplication) -> Bool {
  guard inputProcessIsRunning(Int64(app.processIdentifier)), let executable = app.executableURL else { return false }
  // NOTICE: libproc.h's PROC_PIDPATHINFO_MAXSIZE is 4*MAXPATHLEN; the SDK macro
  // is not imported into Swift. Keep this aligned with sys/proc_info.h.
  var path = [CChar](repeating: 0, count: 4 * Int(MAXPATHLEN))
  let capacity = UInt32(path.count)
  guard proc_pidpath(app.processIdentifier, &path, capacity) > 0 else { return false }
  return URL(fileURLWithPath: String(cString: path)).resolvingSymlinksInPath() == executable.resolvingSymlinksInPath()
}

// Native process lookup does not launch an application or require a visible window.
// 0 means not running; -1 means ambiguous. Rust maps these to typed errors.
func running_application_pid(bundle_id: RustString) -> Int64 {
  let bundleId = bundle_id.toString()
  let observed = NSWorkspace.shared.runningApplications.filter { $0.bundleIdentifier == bundleId }
    + NSRunningApplication.runningApplications(withBundleIdentifier: bundleId)
  inputApplicationsLock.lock()
  defer { inputApplicationsLock.unlock() }
  inputApplications = inputApplications.filter { inputProcessIsRunning(Int64($0.key)) }
  for app in observed { inputApplications[app.processIdentifier] = app }
  let apps = inputApplications.values.filter { $0.bundleIdentifier == bundleId && inputApplicationMatchesProcess($0) }
  if apps.count > 1 { return -1 }
  return apps.first.map { Int64($0.processIdentifier) } ?? 0
}

// Shared preparation validates before activation. Application scope uses 0 and
// requires no AX window; exact-window foreground input additionally needs AX.
func validate_input_target(pid: Int64, window_number: Int64, require_window_focus: Bool) -> NativeActionResponse {
  guard AXIsProcessTrusted() else {
    return nativeActionError("accessibility permission denied", "grant Accessibility permission")
  }
  guard inputProcessIsRunning(pid) else {
    return nativeActionError("target is not a running application", "resolve the target again")
  }
  if window_number == 0 { return nativeActionOk() }
  guard window_number > 0, window_number <= Int64(UInt32.max),
        let windows = CGWindowListCopyWindowInfo(.optionIncludingWindow, CGWindowID(window_number)) as? [[String: Any]],
        windows.contains(where: {
          ($0[kCGWindowNumber as String] as? NSNumber)?.int64Value == window_number &&
          ($0[kCGWindowOwnerPID as String] as? NSNumber)?.int64Value == pid
        }) else {
    return nativeActionError("target window or owner is stale", "resolve the target again")
  }
  if require_window_focus {
    let appElement = AXUIElementCreateApplication(pid_t(pid))
    guard windowAxElementArrayAttribute(appElement, kAXWindowsAttribute as String)
      .contains(where: { windowAxCgWindowId($0) == window_number }) else {
      return nativeActionError("target window could not be resolved through AX", "resolve a current window with accessible focus support")
    }
  }
  return nativeActionOk()
}

// Activation is owned by the existing Rust input preparation lifecycle. Raise
// an exact window only when requested, then observe the required focus predicate.
func confirm_input_focus(pid: Int64, window_number: Int64) -> NativeActionResponse {
  guard inputProcessIsRunning(pid) else {
    return nativeActionError("target is not a running application", "resolve the target again")
  }
  let appElement = AXUIElementCreateApplication(pid_t(pid))
  if window_number != 0 {
    guard let window = windowAxElementArrayAttribute(appElement, kAXWindowsAttribute as String)
      .first(where: { windowAxCgWindowId($0) == window_number }) else {
      return nativeActionError("target window could not be resolved through AX", "resolve the target again")
    }
    let raise = AXUIElementPerformAction(window, kAXRaiseAction as CFString)
    if raise != .success {
      return nativeActionError("target window raise failed: \(raise.rawValue)", "focus the target window and retry")
    }
  }
  let deadline = ProcessInfo.processInfo.systemUptime + 1.0
  repeat {
    if !inputProcessIsRunning(pid) { break }
    // Read current focus from AX rather than the workspace's cached frontmost app.
    if windowAxBoolAttribute(appElement, kAXFrontmostAttribute as String) {
      if window_number == 0 { return nativeActionOk() }
      if let focused = windowAxElementAttribute(appElement, kAXFocusedWindowAttribute as String),
         windowAxCgWindowId(focused) == window_number { return nativeActionOk() }
    }
    // Observe until ready; never repeatedly activate or assume a fixed sleep proves focus.
    RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.01))
  } while ProcessInfo.processInfo.systemUptime < deadline
  return nativeActionError("target activation or window focus was not confirmed", "resolve and focus the target again")
}

/// The window's size and minimized state as its application reports them over
/// Accessibility. Unlike `SCWindow.frame` and `kCGWindowBounds`, this does not
/// shrink while Mission Control, App Exposé or a minimize animation shows the
/// window, so window capture uses it to tell a real resize from a transient
/// presentation transform.
func window_ax_size(pid: Int64, window_number: Int64) -> NativeWindowAxSizeResponse {
  let appElement = AXUIElementCreateApplication(pid_t(pid))
  let windows = windowAxElementArrayAttribute(appElement, kAXWindowsAttribute as String)
  guard let window = windows.first(where: { windowAxCgWindowId($0) == window_number }),
        let size = windowAxSizeAttribute(window, kAXSizeAttribute as String)
  else {
    return NativeWindowAxSizeResponse(found: false, width: 0, height: 0, minimized: false)
  }
  return NativeWindowAxSizeResponse(
    found: true,
    width: Double(size.width),
    height: Double(size.height),
    minimized: windowAxBoolAttribute(window, kAXMinimizedAttribute as String)
  )
}
