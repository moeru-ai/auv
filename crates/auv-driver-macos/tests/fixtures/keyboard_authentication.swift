import AppKit
import Darwin

private var calls: [String] = []
private var copyFails = false
private var factoryFails = false
private let targetPID: pid_t = 1234

private func copyRecord(_ event: CGEvent, _ buffer: UnsafeMutableRawPointer, _ size: UInt32) -> Int32 {
  calls.append("copy")
  precondition(size == 248)
  precondition(event.getIntegerValueField(.keyboardEventKeycode) == 12)
  buffer.storeBytes(of: UInt64(0x1234), as: UInt64.self)
  return copyFails ? 1 : 0
}

private func attach(_ event: CGEvent, _ message: AnyObject) {
  calls.append("attach")
  precondition(message is NSObject)
}

private func post(_ pid: pid_t, _ event: CGEvent) {
  precondition(pid == targetPID)
  calls.append("post")
}

private class Factory: NSObject {
  @objc(messageWithEventRecord:pid:version:)
  class func make(_ record: UnsafeMutableRawPointer, pid: Int32, version: UInt32) -> NSObject? {
    calls.append("factory")
    precondition(pid == targetPID && version == 0)
    precondition(record.load(as: UInt64.self) == 0x1234)
    return factoryFails ? nil : NSObject()
  }
}

private class WrongFactory: NSObject {
  @objc(messageWithEventRecord:pid:version:)
  class func make(_ record: UnsafeMutableRawPointer, pid: Double, version: UInt32) -> NSObject? { nil }
}

private func lookup(_ name: String) -> UnsafeMutableRawPointer? {
  switch name {
  case "SLEventGetEventRecord":
    return unsafeBitCast(copyRecord as KeyboardEventPosting.CopyRecord, to: UnsafeMutableRawPointer.self)
  case "SLEventSetAuthenticationMessage":
    return unsafeBitCast(attach as KeyboardEventPosting.SetAuthentication, to: UnsafeMutableRawPointer.self)
  default: return nil
  }
}

@main
struct ContractTests {
  static func main() {
    let event = CGEvent(keyboardEventSource: nil, virtualKey: 12, keyDown: true)!
    let poster = KeyboardEventPosting(lookup: lookup, messageClass: Factory.self, postToPid: post)!

    // Authentication must be attached before the one platform-facing submission.
    precondition(poster.post(event, to: targetPID))
    precondition(calls == ["copy", "factory", "attach", "post"], "\(calls)")

    // Missing symbols/class/selector must not invoke Objective-C or submit input.
    calls = []
    precondition(KeyboardEventPosting(lookup: { _ in nil }, messageClass: Factory.self, postToPid: post) == nil)
    precondition(KeyboardEventPosting(lookup: lookup, messageClass: nil, postToPid: post) == nil)
    precondition(KeyboardEventPosting(lookup: lookup, messageClass: NSObject.self, postToPid: post) == nil)
    precondition(KeyboardEventPosting(lookup: lookup, messageClass: WrongFactory.self, postToPid: post) == nil)
    precondition(calls.isEmpty)

    // No partially prepared event may reach the private post. False permits the
    // caller's original public route, rather than replay after an attempted post.
    copyFails = true
    precondition(!poster.post(event, to: targetPID))
    precondition(calls == ["copy"])
    copyFails = false
    calls = []
    factoryFails = true
    precondition(!poster.post(event, to: targetPID))
    precondition(calls == ["copy", "factory"])
    factoryFails = false

    // Exercise the real native record copy, factory, and setter, replacing only
    // the external post. No GUI input is emitted by this executable.
    let handle = dlopen("/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight", RTLD_LAZY)!
    calls = []
    if let native = KeyboardEventPosting(
      lookup: { dlsym(handle, $0) },
      messageClass: NSClassFromString("SLSEventAuthenticationMessage"),
      postToPid: post
    ) {
      precondition(native.post(event, to: targetPID), "native authentication preparation failed")
      precondition(calls == ["post"])
      print("native authentication prepared; submission intercepted")
    } else {
      print("native authentication unavailable on this host; injected boundary tests passed")
    }
    print("authentication ordering, single submission, missing API, ABI mismatch, and preparation failure passed")
  }
}
