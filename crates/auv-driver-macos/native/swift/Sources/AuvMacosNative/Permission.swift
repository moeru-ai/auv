import ApplicationServices
import CoreGraphics
import Foundation

func probe_permissions() -> NativePermissionProbeResponse {
  NativePermissionProbeResponse(
    screen_recording: CGPreflightScreenCaptureAccess()
      ? NativePermissionStatus.Granted
      : NativePermissionStatus.Missing,
    accessibility: AXIsProcessTrusted()
      ? NativePermissionStatus.Granted
      : NativePermissionStatus.Missing
  )
}
