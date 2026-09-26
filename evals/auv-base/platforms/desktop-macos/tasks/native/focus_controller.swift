import AppKit
import Carbon
import Darwin

// Test-only port of CUA's 605d358a activate_without_raise. The byte layout and
// signatures are private OS contracts, bounded to this opt-in receiver probe.
// https://github.com/trycua/cua/blob/605d358a48ad938a41b384a8f23909153eb73f78/libs/cua-driver/rust/crates/platform-macos/src/input/skylight.rs#L492
typealias GetFront = @convention(c) (UnsafeMutableRawPointer) -> Int32
typealias PostRecord = @convention(c) (UnsafeRawPointer, UnsafeRawPointer) -> Int32
typealias MainConnection = @convention(c) () -> UInt32
typealias WindowOwner = @convention(c) (UInt32, UInt32, UnsafeMutablePointer<UInt32>) -> Int32
typealias ConnectionPSN = @convention(c) (UInt32, UnsafeMutableRawPointer) -> Int32
typealias ProcessPSN = @convention(c) (Int32, UnsafeMutableRawPointer) -> Int32
typealias PostEvent = @convention(c) (Int32, CGEvent) -> Void

let sky = dlopen("/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight", RTLD_LAZY | RTLD_GLOBAL)!

func symbol<T>(_ name: String, as type: T.Type) -> T {
  guard let pointer = dlsym(sky, name) else {
    fatalError("missing private API: \(name)")
  }

  return unsafeBitCast(pointer, to: T.self)
}

let getFront = symbol("_SLPSGetFrontProcess", as: GetFront.self)
let postRecord = symbol("SLPSPostEventRecordTo", as: PostRecord.self)
let mainConnection = symbol("CGSMainConnectionID", as: MainConnection.self)
let windowOwner = symbol("SLSGetWindowOwner", as: WindowOwner.self)
let connectionPSN = symbol("SLSGetConnectionPSN", as: ConnectionPSN.self)

var trackedWindows: Set<Int> = []
var caseId = "startup"
var lastState = ""

let logURL = URL(fileURLWithPath: CommandLine.arguments[1])
FileManager.default.createFile(atPath: logURL.path, contents: nil)
let logHandle = try! FileHandle(forWritingTo: logURL)

func frontPSN() -> [UInt32] {
  var psn: [UInt32] = [0, 0]
  let result = psn.withUnsafeMutableBytes { getFront($0.baseAddress!) }
  precondition(result == 0, "get front failed: \(result)")

  return psn
}

func psnForWindow(pid: Int32, window: UInt32) -> [UInt32] {
  var psn: [UInt32] = [0, 0]
  var owner: UInt32 = 0

  if windowOwner(mainConnection(), window, &owner) == 0, owner != 0 {
    let result = psn.withUnsafeMutableBytes { connectionPSN(owner, $0.baseAddress!) }
    if result == 0 {
      return psn
    }
  }

  // NOTICE: Swift marks this legacy declaration unavailable. CUA also resolves
  // it dynamically; it is used only if the modern window-owner route failed.
  guard let pointer = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "GetProcessForPID") else {
    fatalError("cannot resolve target process")
  }

  let getProcess = unsafeBitCast(pointer, to: ProcessPSN.self)
  let result = psn.withUnsafeMutableBytes { getProcess(pid, $0.baseAddress!) }
  precondition(result == 0)

  return psn
}

func noRaise(pid: Int32, window: UInt32) -> [String: Any] {
  let previous = frontPSN()
  let target = psnForWindow(pid: pid, window: window)

  return focusTransition(previous: previous, target: target, window: window)
}

func focusTransition(previous: [UInt32], target: [UInt32], window: UInt32) -> [String: Any] {
  var record = [UInt8](repeating: 0, count: 248)
  record[0x04] = 0xF8
  record[0x08] = 0x0D
  for index in 0..<4 {
    record[0x3C + index] = UInt8(truncatingIfNeeded: window >> (index * 8))
  }

  record[0x8A] = 0x02
  let defocus = previous.withUnsafeBytes { psn in
    record.withUnsafeBytes { postRecord(psn.baseAddress!, $0.baseAddress!) }
  }

  record[0x8A] = 0x01
  let focus = target.withUnsafeBytes { psn in
    record.withUnsafeBytes { postRecord(psn.baseAddress!, $0.baseAddress!) }
  }

  return [
    "previousPSN": previous,
    "targetPSN": target,
    "defocusStatus": defocus,
    "focusStatus": focus,
  ]
}

func makeKeyRecords(pid: Int32, window: UInt32) -> [Int32] {
  // Differential probe: CUA's make_exact_window_key record pair, deliberately
  // omitting its set-front call to isolate the key-window transition.
  let target = psnForWindow(pid: pid, window: window)

  return [UInt8(0x01), 0x02].map { kind in
    var record = [UInt8](repeating: 0, count: 248)
    record[0x04] = 0xF8
    record[0x08] = kind
    record[0x3A] = 0x10

    for index in 0x20..<0x30 {
      record[index] = 0xFF
    }

    for index in 0..<4 {
      record[0x3C + index] = UInt8(truncatingIfNeeded: window >> (index * 8))
    }

    return target.withUnsafeBytes { psn in
      record.withUnsafeBytes { postRecord(psn.baseAddress!, $0.baseAddress!) }
    }
  }
}

func menuSelectAll(pid: Int32) {
  // CUA hotkey_no_auth: a base-key pair with Command flags, no authentication
  // envelope. This is an explicit transport control, not AUV production input.
  let post = symbol("SLEventPostToPid", as: PostEvent.self)
  let source = CGEventSource(stateID: .hidSystemState)

  for down in [true, false] {
    let event = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: down)!
    event.flags = .maskCommand
    post(pid, event)
    Thread.sleep(forTimeInterval: 0.008)
  }
}

func state() -> [String: Any] {
  let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
  let order = windows.compactMap { item -> Int? in
    guard let id = item[kCGWindowNumber as String] as? Int,
          trackedWindows.contains(id)
    else {
      return nil
    }

    return id
  }

  return [
    "caseId": caseId,
    "workspaceFrontPid": NSWorkspace.shared.frontmostApplication?.processIdentifier ?? 0,
    "windowServerFrontPSN": frontPSN(),
    "windowOrder": order,
  ]
}

func sample() {
  var value = state()
  let encoded = String(data: try! JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]), encoding: .utf8)!

  if encoded != lastState {
    value["timeMs"] = Date().timeIntervalSince1970 * 1000
    logHandle.write(try! JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]))
    logHandle.write(Data([10]))
    lastState = encoded
  }
}

func handle(_ request: [String: Any]) -> [String: Any] {
  let command = request["command"] as! String

  switch command {
  case "track":
    trackedWindows = Set(request["windows"] as! [Int])
    return state()

  case "case":
    caseId = request["id"] as! String
    sample()
    return state()

  case "activate":
    let pid = (request["pid"] as! NSNumber).int32Value
    return ["accepted": NSRunningApplication(processIdentifier: pid)?.activate(options: []) ?? false]

  case "no_raise":
    return noRaise(
      pid: (request["pid"] as! NSNumber).int32Value,
      window: (request["window"] as! NSNumber).uint32Value
    )

  case "make_key":
    return [
      "statuses": makeKeyRecords(
        pid: (request["pid"] as! NSNumber).int32Value,
        window: (request["window"] as! NSNumber).uint32Value
      )
    ]

  case "restore_records":
    let oldPid = (request["fromPid"] as! NSNumber).int32Value
    let oldWindow = (request["fromWindow"] as! NSNumber).uint32Value
    let pid = (request["pid"] as! NSNumber).int32Value
    let window = (request["window"] as! NSNumber).uint32Value

    // WindowServer can still name the original app while its AppKit state is
    // inactive. Address the actually activated receiver explicitly on restore.
    var result = focusTransition(
      previous: psnForWindow(pid: oldPid, window: oldWindow),
      target: psnForWindow(pid: pid, window: window),
      window: window
    )
    result["keyStatuses"] = makeKeyRecords(pid: pid, window: window)

    return result

  case "menu_select_all":
    menuSelectAll(pid: (request["pid"] as! NSNumber).int32Value)
    return ["posted": true]

  case "source":
    if let id = request["id"] as? String {
      let sources = TISCreateInputSourceList([kTISPropertyInputSourceID: id] as CFDictionary, false).takeRetainedValue() as! [TISInputSource]
      precondition(!sources.isEmpty && TISSelectInputSource(sources[0]) == noErr)
    }

    let source = TISCopyCurrentKeyboardInputSource().takeRetainedValue()
    let property = TISGetInputSourceProperty(source, kTISPropertyInputSourceID)!
    return ["id": Unmanaged<CFString>.fromOpaque(property).takeUnretainedValue() as String]

  case "windows":
    let pid = (request["pid"] as! NSNumber).int32Value
    let windows = CGWindowListCopyWindowInfo(.optionOnScreenOnly, kCGNullWindowID) as? [[String: Any]] ?? []

    return [
      "windows": windows.filter { ($0[kCGWindowOwnerPID as String] as? Int32) == pid }.map {
        [
          "id": $0[kCGWindowNumber as String] ?? 0,
          "title": $0[kCGWindowName as String] ?? "",
          "layer": $0[kCGWindowLayer as String] ?? 0,
        ]
      }
    ]

  default:
    return state()
  }
}

// Keep AppKit notifications and state observations alive between requests.
_ = NSApplication.shared
Timer.scheduledTimer(withTimeInterval: 0.01, repeats: true) { _ in sample() }

DispatchQueue.global().async {
  while let line = readLine() {
    let request = try! JSONSerialization.jsonObject(with: Data(line.utf8)) as! [String: Any]

    DispatchQueue.main.async {
      let data = try! JSONSerialization.data(withJSONObject: handle(request), options: [.sortedKeys])
      print(String(data: data, encoding: .utf8)!)
      fflush(stdout)
    }
  }

  DispatchQueue.main.async { exit(0) }
}

RunLoop.main.run()
