import AppKit
import CoreGraphics
import Darwin
import ObjectiveC

typealias SLEventPostToPidFn = @convention(c) (pid_t, CGEvent) -> Void

// Keep the framework loaded for the lifetime of its cached function pointers and classes.
private let skyLightHandle = dlopen(
  "/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight", RTLD_LAZY | RTLD_GLOBAL
)

// Shared by the existing mouse route and authenticated keyboard submission.
let slEventPostToPid: SLEventPostToPidFn? = {
  _ = skyLightHandle
  guard let symbol = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "SLEventPostToPid") else { return nil }
  return unsafeBitCast(symbol, to: SLEventPostToPidFn.self)
}()

/// Resolves and prepares the complete private keyboard capability before any input is sent.
/// The injected lookup and class are native API boundaries, also exercised without posting
/// to the desktop by the standalone native contract tests.
struct KeyboardEventPosting {
  typealias CopyRecord = @convention(c) (CGEvent, UnsafeMutableRawPointer, UInt32) -> Int32
  typealias SetAuthentication = @convention(c) (CGEvent, AnyObject) -> Void
  typealias MakeAuthentication = @convention(c) (
    AnyObject, Selector, UnsafeMutableRawPointer, Int32, UInt32
  ) -> Unmanaged<AnyObject>?

  private let copyRecord: CopyRecord
  private let setAuthentication: SetAuthentication
  private let postToPid: SLEventPostToPidFn
  private let makeAuthentication: MakeAuthentication
  private let messageClass: AnyClass
  private let factorySelector: Selector

  static let resolved: KeyboardEventPosting? = {
    guard let handle = skyLightHandle, let postToPid = slEventPostToPid else { return nil }
    return KeyboardEventPosting(
      lookup: { dlsym(handle, $0) },
      messageClass: NSClassFromString("SLSEventAuthenticationMessage"),
      postToPid: postToPid
    )
  }()

  init?(lookup: (String) -> UnsafeMutableRawPointer?, messageClass: AnyClass?, postToPid: @escaping SLEventPostToPidFn) {
    let selector = NSSelectorFromString("messageWithEventRecord:pid:version:")
    // NOTICE: The class can exist without this factory (macOS 14). Interning a
    // selector is not an availability check; never send an unrecognized selector.
    // Reference: CUA `platform-macos/src/input/skylight.rs` at d1a01f8580d5963702427b9e110fbcd98c39fac3.
    guard let messageClass,
      let factory = class_getClassMethod(messageClass, selector),
      method_getNumberOfArguments(factory) == 5,
      let getter = lookup("SLEventGetEventRecord"),
      let setter = lookup("SLEventSetAuthenticationMessage")
    else { return nil }

    // Check the Objective-C ABI before converting the IMP to a typed C call.
    for (index, expected) in [(UInt32(2), "^"), (3, "i"), (4, "I")] {
      guard let encoding = method_copyArgumentType(factory, index) else { return nil }
      defer { free(encoding) }
      guard String(cString: encoding).hasPrefix(expected) else { return nil }
    }
    let returnType = method_copyReturnType(factory)
    defer { free(returnType) }
    guard String(cString: returnType) == "@" else { return nil }

    self.copyRecord = unsafeBitCast(getter, to: CopyRecord.self)
    self.setAuthentication = unsafeBitCast(setter, to: SetAuthentication.self)
    self.postToPid = postToPid
    self.makeAuthentication = unsafeBitCast(method_getImplementation(factory), to: MakeAuthentication.self)
    self.messageClass = messageClass
    self.factorySelector = selector
  }

  /// False means preparation failed before posting; true means one submission,
  /// never receiver acknowledgement. Callers may use public posting only on false.
  func post(_ event: CGEvent, to pid: pid_t) -> Bool {
    return autoreleasepool {
      // NOTICE: `SLEventGetEventRecord(event, buffer, size)` asserts on a wrong size.
      // Field 50 and the getter were inspected on macOS 26.3 arm64: 248-byte record,
      // zero CGError on copy. Reject other layouts instead of guessing object offsets.
      // See `docs/ai/references/driver/2026-09-23-background-keyboard-authentication.md`.
      let recordSize = 248
      guard let lengthField = CGEventField(rawValue: 50),
        event.getIntegerValueField(lengthField) == Int64(recordSize)
      else { return false }
      let record = UnsafeMutableRawPointer.allocate(byteCount: recordSize, alignment: 16)
      defer { record.deallocate() }
      guard copyRecord(event, record, UInt32(recordSize)) == 0,
        let message = makeAuthentication(messageClass, factorySelector, record, pid, 0)?.takeUnretainedValue()
      else { return false }

      // The factory returns a +0 object. Keep it and the copied record alive through
      // attachment and submission; the source CGEvent owns any pointers in the record.
      withExtendedLifetime(message) {
        setAuthentication(event, message)
        postToPid(pid, event)
      }
      return true
    }
  }
}

func postKeyboardEvent(_ event: CGEvent, to pid: pid_t) {
  if KeyboardEventPosting.resolved?.post(event, to: pid) == true { return }
  // Preserve the existing public route when authenticated preparation is unavailable.
  // No replay is attempted after a private post. NOTICE: Per-toolkit routing and
  // public authentication-status metadata need a concrete consumer and remain deferred;
  // see the background-keyboard-authentication reference. Submission stays unverified.
  event.postToPid(pid)
}
