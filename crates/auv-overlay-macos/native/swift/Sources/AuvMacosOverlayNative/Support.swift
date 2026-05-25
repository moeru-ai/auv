import Foundation

func nativeActionOk() -> NativeActionResponse {
  NativeActionResponse(ok: true, error_message: nil, recovery_hint: nil)
}

func nativeActionError(_ message: String, _ recovery: String) -> NativeActionResponse {
  NativeActionResponse(
    ok: false,
    error_message: message.intoRustString(),
    recovery_hint: recovery.intoRustString()
  )
}
