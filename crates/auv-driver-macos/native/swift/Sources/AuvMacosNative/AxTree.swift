import AppKit
import ApplicationServices
import Foundation

private func emptyAxTreeResponse(message: String, recovery: String) -> NativeAxTreeResponse {
  NativeAxTreeResponse(
    observed_at: nativeNowIso8601(),
    app_name: "".intoRustString(),
    bundle_id: "".intoRustString(),
    pid: 0,
    window_title: "".intoRustString(),
    root_role: "".intoRustString(),
    depths: RustVec<Int64>(),
    paths: RustVec<RustString>(),
    roles: RustVec<RustString>(),
    subroles: RustVec<RustString>(),
    titles: RustVec<RustString>(),
    descriptions: RustVec<RustString>(),
    helps: RustVec<RustString>(),
    identifiers: RustVec<RustString>(),
    placeholders: RustVec<RustString>(),
    values: RustVec<RustString>(),
    focused_values: RustVec<Bool>(),
    x_values: RustVec<Int64>(),
    y_values: RustVec<Int64>(),
    width_values: RustVec<Int64>(),
    height_values: RustVec<Int64>(),
    error_message: message.intoRustString(),
    recovery_hint: recovery.intoRustString()
  )
}

private func axMatches(_ app: NSRunningApplication, query: String) -> Bool {
  let name = app.localizedName?.lowercased() ?? ""
  let bundleId = app.bundleIdentifier?.lowercased() ?? ""
  return name == query || name.contains(query) || bundleId == query || bundleId.contains(query)
}

private func axTargetApp(_ query: String) -> NSRunningApplication? {
  let running = NSWorkspace.shared.runningApplications.filter { !$0.isTerminated }
  if query.isEmpty {
    return NSWorkspace.shared.frontmostApplication ?? running.first
  }
  return running.first(where: { axMatches($0, query: query) })
}

private func axAttributeValue(_ element: AXUIElement, _ attribute: String) -> CFTypeRef? {
  var value: CFTypeRef?
  let result = AXUIElementCopyAttributeValue(element, attribute as CFString, &value)
  guard result == .success else { return nil }
  return value
}

private func axValueAttribute(_ element: AXUIElement, _ attribute: String) -> AXValue? {
  guard let rawValue = axAttributeValue(element, attribute) else { return nil }
  guard CFGetTypeID(rawValue) == AXValueGetTypeID() else { return nil }
  return unsafeBitCast(rawValue, to: AXValue.self)
}

private func axElementAttribute(_ element: AXUIElement, _ attribute: String) -> AXUIElement? {
  guard let rawValue = axAttributeValue(element, attribute) else { return nil }
  guard CFGetTypeID(rawValue) == AXUIElementGetTypeID() else { return nil }
  return unsafeBitCast(rawValue, to: AXUIElement.self)
}

private func axElementArrayAttribute(_ element: AXUIElement, _ attribute: String) -> [AXUIElement] {
  guard let rawValue = axAttributeValue(element, attribute) else { return [] }
  guard let array = rawValue as? NSArray else { return [] }
  return array.compactMap { item in
    let value = item as CFTypeRef
    guard CFGetTypeID(value) == AXUIElementGetTypeID() else { return nil }
    return unsafeBitCast(value, to: AXUIElement.self)
  }
}

private func axStringAttribute(_ element: AXUIElement, _ attribute: String) -> String {
  if let stringValue = axAttributeValue(element, attribute) as? String {
    return nativeSanitize(stringValue)
  }
  if let numberValue = axAttributeValue(element, attribute) as? NSNumber {
    return numberValue.stringValue
  }
  return ""
}

private func axBoolAttribute(_ element: AXUIElement, _ attribute: String) -> Bool {
  guard let rawValue = axAttributeValue(element, attribute) else { return false }
  if CFGetTypeID(rawValue) == CFBooleanGetTypeID() {
    return CFBooleanGetValue((rawValue as! CFBoolean))
  }
  if let numberValue = rawValue as? NSNumber {
    return numberValue.boolValue
  }
  return false
}

private func axChildren(_ element: AXUIElement) -> [AXUIElement] {
  axElementArrayAttribute(element, kAXChildrenAttribute as String)
}

private func axPointAttribute(_ element: AXUIElement, _ attribute: String) -> CGPoint? {
  guard let value = axValueAttribute(element, attribute) else { return nil }
  guard AXValueGetType(value) == .cgPoint else { return nil }
  var point = CGPoint.zero
  guard AXValueGetValue(value, .cgPoint, &point) else { return nil }
  return point
}

private func axSizeAttribute(_ element: AXUIElement, _ attribute: String) -> CGSize? {
  guard let value = axValueAttribute(element, attribute) else { return nil }
  guard AXValueGetType(value) == .cgSize else { return nil }
  var size = CGSize.zero
  guard AXValueGetValue(value, .cgSize, &size) else { return nil }
  return size
}

private func axBounds(_ element: AXUIElement) -> (Int64, Int64, Int64, Int64) {
  let point = axPointAttribute(element, kAXPositionAttribute as String) ?? .zero
  let size = axSizeAttribute(element, kAXSizeAttribute as String) ?? .zero
  return (
    Int64(point.x.rounded()),
    Int64(point.y.rounded()),
    Int64(size.width.rounded()),
    Int64(size.height.rounded())
  )
}

private func axFirstWindow(_ appElement: AXUIElement) -> AXUIElement? {
  if let focused = axElementAttribute(appElement, kAXFocusedWindowAttribute as String) {
    return focused
  }
  return axElementArrayAttribute(appElement, kAXWindowsAttribute as String).first
}

private struct AxPathResolutionFailure: Error {
  let message: String
  let recovery: String
}

private func axObservedPathIndices(path: String, operation: String, retry: String) -> Result<[Int], AxPathResolutionFailure> {
  let segments = path.split(separator: ".").map(String.init)
  guard segments.first == "0" else {
    return .failure(AxPathResolutionFailure(
      message: "AX \(operation) path must begin with 0; got \(path)",
      recovery: "capture a fresh AX tree and retry \(retry)"
    ))
  }

  var indices: [Int] = []
  for (offset, segment) in segments.dropFirst().enumerated() {
    guard let index = Int(segment), index >= 0 else {
      return .failure(AxPathResolutionFailure(
        message: "AX \(operation) path segment \(segment) at offset \(offset) is not a non-negative integer",
        recovery: "capture a fresh AX tree and retry \(retry)"
      ))
    }
    indices.append(index)
  }
  return .success(indices)
}

private func axResolveObservedPath(
  pid: pid_t,
  path: String,
  expectedRole: String,
  operation: String,
  retry: String
) -> Result<AXUIElement, AxPathResolutionFailure> {
  let indices: [Int]
  switch axObservedPathIndices(path: path, operation: operation, retry: retry) {
  case .success(let parsed):
    indices = parsed
  case .failure(let failure):
    return .failure(failure)
  }

  let appElement = AXUIElementCreateApplication(pid)
  var current = axFirstWindow(appElement) ?? appElement
  for (offset, index) in indices.enumerated() {
    let children = axChildren(current)
    guard children.indices.contains(index) else {
      return .failure(AxPathResolutionFailure(
        message: "AX \(operation) path index \(index) is out of range at offset \(offset); element has \(children.count) child(ren)",
        recovery: "the AX tree likely shifted since observation; capture a fresh tree and retry \(retry)"
      ))
    }
    current = children[index]
  }

  let actualRole = axStringAttribute(current, kAXRoleAttribute as String)
  guard expectedRole.isEmpty || actualRole == expectedRole else {
    return .failure(AxPathResolutionFailure(
      message: "AX \(operation) expected role \(expectedRole) at path \(path), got \(actualRole)",
      recovery: "the AX tree likely shifted since observation; capture a fresh tree and retry \(retry)"
    ))
  }

  return .success(current)
}

func capture_ax_tree(request: NativeAxTreeRequest) -> NativeAxTreeResponse {
  let appQuery = request.app.toString().lowercased().trimmingCharacters(in: .whitespacesAndNewlines)
  let maxDepth = Int(request.max_depth)
  let maxChildren = Int(request.max_children)

  guard let app = axTargetApp(appQuery) else {
    return emptyAxTreeResponse(
      message: "could not resolve target macOS app for AX tree observation",
      recovery: "activate the target app or pass a matching app name or bundle id"
    )
  }

  let appElement = AXUIElementCreateApplication(app.processIdentifier)
  let rootElement = axFirstWindow(appElement) ?? appElement
  let windowTitle = axStringAttribute(rootElement, kAXTitleAttribute as String)
  let rootRole = axStringAttribute(rootElement, kAXRoleAttribute as String)

  var depths: [Int64] = []
  var paths: [String] = []
  var roles: [String] = []
  var subroles: [String] = []
  var titles: [String] = []
  var descriptions: [String] = []
  var helps: [String] = []
  var identifiers: [String] = []
  var placeholders: [String] = []
  var values: [String] = []
  var focusedValues: [Bool] = []
  var xValues: [Int64] = []
  var yValues: [Int64] = []
  var widthValues: [Int64] = []
  var heightValues: [Int64] = []

  func appendNode(_ element: AXUIElement, depth: Int, path: String) {
    let frame = axBounds(element)
    depths.append(Int64(depth))
    paths.append(path)
    roles.append(axStringAttribute(element, kAXRoleAttribute as String))
    subroles.append(axStringAttribute(element, kAXSubroleAttribute as String))
    titles.append(axStringAttribute(element, kAXTitleAttribute as String))
    descriptions.append(axStringAttribute(element, kAXDescriptionAttribute as String))
    helps.append(axStringAttribute(element, kAXHelpAttribute as String))
    identifiers.append(axStringAttribute(element, kAXIdentifierAttribute as String))
    placeholders.append(axStringAttribute(element, "AXPlaceholderValue"))
    values.append(axStringAttribute(element, kAXValueAttribute as String))
    focusedValues.append(axBoolAttribute(element, kAXFocusedAttribute as String))
    xValues.append(frame.0)
    yValues.append(frame.1)
    widthValues.append(frame.2)
    heightValues.append(frame.3)
  }

  func walk(_ element: AXUIElement, depth: Int, path: String) {
    appendNode(element, depth: depth, path: path)
    if depth >= maxDepth {
      return
    }

    let visibleChildren = Array(axChildren(element).prefix(maxChildren))
    for (index, child) in visibleChildren.enumerated() {
      walk(child, depth: depth + 1, path: "\(path).\(index)")
    }
  }

  walk(rootElement, depth: 0, path: "0")

  return NativeAxTreeResponse(
    observed_at: nativeNowIso8601(),
    app_name: nativeSanitize(app.localizedName).intoRustString(),
    bundle_id: nativeSanitize(app.bundleIdentifier).intoRustString(),
    pid: Int64(app.processIdentifier),
    window_title: windowTitle.intoRustString(),
    root_role: rootRole.intoRustString(),
    depths: nativeVec(depths),
    paths: nativeStringVec(paths),
    roles: nativeStringVec(roles),
    subroles: nativeStringVec(subroles),
    titles: nativeStringVec(titles),
    descriptions: nativeStringVec(descriptions),
    helps: nativeStringVec(helps),
    identifiers: nativeStringVec(identifiers),
    placeholders: nativeStringVec(placeholders),
    values: nativeStringVec(values),
    focused_values: nativeVec(focusedValues),
    x_values: nativeVec(xValues),
    y_values: nativeVec(yValues),
    width_values: nativeVec(widthValues),
    height_values: nativeVec(heightValues),
    error_message: nil,
    recovery_hint: nil
  )
}

func perform_ax_action(request: NativeAxActionRequest) -> NativeAxActionResponse {
  let pid = pid_t(request.pid)
  let pathRaw = request.path.toString()
  let expectedRole = request.expected_role.toString()
  let actionName = request.action_name.toString()

  func actionError(_ message: String, _ recovery: String) -> NativeAxActionResponse {
    NativeAxActionResponse(
      performed_action: "".intoRustString(),
      available_actions: "".intoRustString(),
      role: "".intoRustString(),
      subrole: "".intoRustString(),
      title: "".intoRustString(),
      description: "".intoRustString(),
      identifier: "".intoRustString(),
      error_message: message.intoRustString(),
      recovery_hint: recovery.intoRustString()
    )
  }

  let current: AXUIElement
  switch axResolveObservedPath(pid: pid, path: pathRaw, expectedRole: expectedRole, operation: "action", retry: "the action") {
  case .success(let resolved):
    current = resolved
  case .failure(let failure):
    return actionError(failure.message, failure.recovery)
  }

  let actualRole = axStringAttribute(current, kAXRoleAttribute as String)
  let actualSubrole = axStringAttribute(current, kAXSubroleAttribute as String)
  let actualTitle = axStringAttribute(current, kAXTitleAttribute as String)
  let actualDescription = axStringAttribute(current, kAXDescriptionAttribute as String)
  let actualIdentifier = axStringAttribute(current, kAXIdentifierAttribute as String)

  var actionNames: CFArray?
  let listResult = AXUIElementCopyActionNames(current, &actionNames)
  let actions: [String]
  if listResult == .success, let raw = actionNames as? [String] {
    actions = raw
  } else {
    actions = []
  }
  let availableActions = actions.joined(separator: ",")

  guard actions.contains(actionName) else {
    return NativeAxActionResponse(
      performed_action: "".intoRustString(),
      available_actions: availableActions.intoRustString(),
      role: actualRole.intoRustString(),
      subrole: actualSubrole.intoRustString(),
      title: actualTitle.intoRustString(),
      description: actualDescription.intoRustString(),
      identifier: actualIdentifier.intoRustString(),
      error_message: "AX action target does not support \(actionName)".intoRustString(),
      recovery_hint: "choose a supported AX action from available_actions or match another node".intoRustString()
    )
  }

  let pressResult = AXUIElementPerformAction(current, actionName as CFString)
  guard pressResult == .success else {
    return NativeAxActionResponse(
      performed_action: "".intoRustString(),
      available_actions: availableActions.intoRustString(),
      role: actualRole.intoRustString(),
      subrole: actualSubrole.intoRustString(),
      title: actualTitle.intoRustString(),
      description: actualDescription.intoRustString(),
      identifier: actualIdentifier.intoRustString(),
      error_message: "AXUIElementPerformAction(\(actionName)) returned \(pressResult.rawValue)".intoRustString(),
      recovery_hint: "verify Accessibility permission and retry against a fresh AX tree".intoRustString()
    )
  }

  return NativeAxActionResponse(
    performed_action: actionName.intoRustString(),
    available_actions: availableActions.intoRustString(),
    role: actualRole.intoRustString(),
    subrole: actualSubrole.intoRustString(),
    title: actualTitle.intoRustString(),
    description: actualDescription.intoRustString(),
    identifier: actualIdentifier.intoRustString(),
    error_message: nil,
    recovery_hint: nil
  )
}

func inspect_ax_node(request: NativeAxNodeInspectionRequest) -> NativeAxNodeInspectionResponse {
  let pid = pid_t(request.pid)
  let pathRaw = request.path.toString()
  let expectedRole = request.expected_role.toString()

  func inspectionError(_ message: String, _ recovery: String) -> NativeAxNodeInspectionResponse {
    NativeAxNodeInspectionResponse(
      role: "".intoRustString(),
      available_actions: RustVec<RustString>(),
      available_attributes: RustVec<RustString>(),
      children_count: 0,
      visible_children_count: 0,
      contents_count: 0,
      navigation_children_count: 0,
      error_message: message.intoRustString(),
      recovery_hint: recovery.intoRustString()
    )
  }

  let current: AXUIElement
  switch axResolveObservedPath(pid: pid, path: pathRaw, expectedRole: expectedRole, operation: "inspection", retry: "the inspection") {
  case .success(let resolved):
    current = resolved
  case .failure(let failure):
    return inspectionError(failure.message, failure.recovery)
  }

  let actualRole = axStringAttribute(current, kAXRoleAttribute as String)

  var actionNames: CFArray?
  let actionsResult = AXUIElementCopyActionNames(current, &actionNames)
  guard actionsResult == .success else {
    return inspectionError(
      "AX inspection could not read action names at path \(pathRaw): AX error \(actionsResult.rawValue)",
      "verify Accessibility permission and retry the inspection against a fresh AX tree"
    )
  }
  let availableActions = actionNames as? [String] ?? []

  var attributeNames: CFArray?
  let attributesResult = AXUIElementCopyAttributeNames(current, &attributeNames)
  guard attributesResult == .success else {
    return inspectionError(
      "AX inspection could not read attribute names at path \(pathRaw): AX error \(attributesResult.rawValue)",
      "verify Accessibility permission and retry the inspection against a fresh AX tree"
    )
  }
  let availableAttributes = attributeNames as? [String] ?? []

  func inspectedElementCount(_ attribute: String) -> Result<Int, AxPathResolutionFailure> {
    var rawValue: CFTypeRef?
    let result = AXUIElementCopyAttributeValue(current, attribute as CFString, &rawValue)
    if result == .noValue || result == .attributeUnsupported {
      return .success(0)
    }
    guard result == .success else {
      return .failure(AxPathResolutionFailure(
        message: "AX inspection could not read \(attribute) at path \(pathRaw): AX error \(result.rawValue)",
        recovery: "verify Accessibility permission and retry the inspection against a fresh AX tree"
      ))
    }
    guard let array = rawValue as? NSArray else {
      return .failure(AxPathResolutionFailure(
        message: "AX inspection expected \(attribute) to contain an array at path \(pathRaw)",
        recovery: "capture a fresh AX tree and retry the inspection"
      ))
    }
    return .success(array.reduce(into: 0) { count, item in
      let value = item as CFTypeRef
      if CFGetTypeID(value) == AXUIElementGetTypeID() {
        count += 1
      }
    })
  }

  // Diagnostic only; the owning rationale lives in the Apple Music AX reference note.
  let attributes = [kAXChildrenAttribute as String, "AXVisibleChildren", "AXContents", "AXChildrenInNavigationOrder"]
  var counts: [Int] = []
  for attribute in attributes {
    switch inspectedElementCount(attribute) {
    case .success(let count):
      counts.append(count)
    case .failure(let failure):
      return inspectionError(failure.message, failure.recovery)
    }
  }

  return NativeAxNodeInspectionResponse(
    role: actualRole.intoRustString(),
    available_actions: nativeStringVec(availableActions),
    available_attributes: nativeStringVec(availableAttributes),
    children_count: Int64(counts[0]),
    visible_children_count: Int64(counts[1]),
    contents_count: Int64(counts[2]),
    navigation_children_count: Int64(counts[3]),
    error_message: nil,
    recovery_hint: nil
  )
}

func set_ax_focused(request: NativeAxFocusRequest) -> NativeAxFocusResponse {
  let pid = pid_t(request.pid)
  let pathRaw = request.path.toString()
  let expectedRole = request.expected_role.toString()

  func focusError(_ message: String, _ recovery: String) -> NativeAxFocusResponse {
    NativeAxFocusResponse(
      set_attribute: "".intoRustString(),
      was_already_focused: false,
      role: "".intoRustString(),
      subrole: "".intoRustString(),
      title: "".intoRustString(),
      description: "".intoRustString(),
      identifier: "".intoRustString(),
      placeholder: "".intoRustString(),
      x: 0,
      y: 0,
      width: 0,
      height: 0,
      error_message: message.intoRustString(),
      recovery_hint: recovery.intoRustString()
    )
  }

  let current: AXUIElement
  switch axResolveObservedPath(pid: pid, path: pathRaw, expectedRole: expectedRole, operation: "focus", retry: "the focus request") {
  case .success(let resolved):
    current = resolved
  case .failure(let failure):
    return focusError(failure.message, failure.recovery)
  }

  let actualRole = axStringAttribute(current, kAXRoleAttribute as String)
  let actualSubrole = axStringAttribute(current, kAXSubroleAttribute as String)
  let actualTitle = axStringAttribute(current, kAXTitleAttribute as String)
  let actualDescription = axStringAttribute(current, kAXDescriptionAttribute as String)
  let actualIdentifier = axStringAttribute(current, kAXIdentifierAttribute as String)
  let actualPlaceholder = axStringAttribute(current, kAXPlaceholderValueAttribute as String)
  let bounds = axBounds(current)

  // Check whether the element is currently focused; treat that as a no-op success.
  var alreadyFocused = false
  if let focusedValue = axAttributeValue(current, kAXFocusedAttribute as String) {
    if CFGetTypeID(focusedValue) == CFBooleanGetTypeID() {
      alreadyFocused = CFBooleanGetValue((focusedValue as! CFBoolean))
    }
  }

  if !alreadyFocused {
    let setResult = AXUIElementSetAttributeValue(
      current,
      kAXFocusedAttribute as CFString,
      kCFBooleanTrue
    )
    guard setResult == .success else {
      return focusError(
        "AXUIElementSetAttributeValue(kAXFocusedAttribute) returned \(setResult.rawValue) on role \(actualRole) at path \(pathRaw)",
        "the element may not accept programmatic focus; verify the AX subtree exposes AXFocused as settable, or fall back to debug.focusTextInput"
      )
    }
  }

  return NativeAxFocusResponse(
    set_attribute: "AXFocused".intoRustString(),
    was_already_focused: alreadyFocused,
    role: actualRole.intoRustString(),
    subrole: actualSubrole.intoRustString(),
    title: actualTitle.intoRustString(),
    description: actualDescription.intoRustString(),
    identifier: actualIdentifier.intoRustString(),
    placeholder: actualPlaceholder.intoRustString(),
    x: bounds.0,
    y: bounds.1,
    width: bounds.2,
    height: bounds.3,
    error_message: nil,
    recovery_hint: nil
  )
}
