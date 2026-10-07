import Foundation

func nativeNowIso8601() -> RustString {
  ISO8601DateFormatter().string(from: Date()).intoRustString()
}

func nativeVec<T: Vectorizable>(_ values: [T]) -> RustVec<T> {
  let vector = RustVec<T>()
  for value in values {
    vector.push(value: value)
  }
  return vector
}

func nativeStringVec(_ values: [String]) -> RustVec<RustString> {
  let vector = RustVec<RustString>()
  for value in values {
    vector.push(value: value.intoRustString())
  }
  return vector
}

// One bulk copy into Rust; see NOTICE(swift-bridge-bulk-bytes) in binding.rs.
func nativeByteVec(_ values: [UInt8]) -> RustVec<UInt8> {
  values.withUnsafeBufferPointer { buffer in
    guard let base = buffer.baseAddress else {
      return RustVec<UInt8>()
    }
    return native_byte_vec_from_raw(base, UInt(buffer.count))
  }
}

func nativeSanitize(_ raw: String?) -> String {
  guard let raw else { return "" }
  return raw
    .replacingOccurrences(of: "\t", with: " ")
    .replacingOccurrences(of: "\n", with: " ")
    .replacingOccurrences(of: "\r", with: " ")
    .trimmingCharacters(in: .whitespacesAndNewlines)
}

func nativeSanitize(_ raw: String) -> String {
  nativeSanitize(Optional(raw))
}

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
