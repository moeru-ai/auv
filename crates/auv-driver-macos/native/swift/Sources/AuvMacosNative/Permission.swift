import ApplicationServices
import CoreGraphics
import Foundation
import ScreenCaptureKit

func probe_permissions() -> NativePermissionProbeResponse {
  let screenCaptureKit = probeScreenCaptureKitAccess()
  return NativePermissionProbeResponse(
    screen_recording: CGPreflightScreenCaptureAccess()
      ? NativePermissionStatus.Granted
      : NativePermissionStatus.Missing,
    screen_capture_kit: screenCaptureKit.status,
    screen_capture_kit_error: screenCaptureKit.errorMessage?.intoRustString(),
    accessibility: AXIsProcessTrusted()
      ? NativePermissionStatus.Granted
      : NativePermissionStatus.Missing
  )
}

private struct ScreenCaptureKitProbe {
  let status: NativePermissionStatus
  let errorMessage: String?
}

private func probeScreenCaptureKitAccess() -> ScreenCaptureKitProbe {
  guard #available(macOS 12.3, *) else {
    return ScreenCaptureKitProbe(
      status: .Failed,
      errorMessage: "ScreenCaptureKit permission probing requires macOS 12.3 or newer"
    )
  }

  let semaphore = DispatchSemaphore(value: 0)
  var result = ScreenCaptureKitProbe(status: .Failed, errorMessage: "ScreenCaptureKit returned no result")

  SCShareableContent.getWithCompletionHandler { content, error in
    if let error {
      result = ScreenCaptureKitProbe(status: .Failed, errorMessage: error.localizedDescription)
    } else if content != nil {
      result = ScreenCaptureKitProbe(status: .Granted, errorMessage: nil)
    } else {
      result = ScreenCaptureKitProbe(status: .Failed, errorMessage: "ScreenCaptureKit returned no shareable content")
    }
    semaphore.signal()
  }

  if semaphore.wait(timeout: .now() + .seconds(3)) == .timedOut {
    return ScreenCaptureKitProbe(
      status: .TimedOut,
      errorMessage: "ScreenCaptureKit permission probe timed out after 3 seconds"
    )
  }
  return result
}
