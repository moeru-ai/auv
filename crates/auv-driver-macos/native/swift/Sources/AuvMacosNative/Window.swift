import AppKit
import CoreGraphics
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
