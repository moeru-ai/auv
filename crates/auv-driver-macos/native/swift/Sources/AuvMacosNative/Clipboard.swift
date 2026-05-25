import AppKit
import Foundation

func capture_clipboard() -> NativeClipboardSnapshotResponse {
  let pasteboard = NSPasteboard.general
  let payloadItems = (pasteboard.pasteboardItems ?? []).map { item in
    item.types.reduce(into: [String: String]()) { result, type in
      if let data = item.data(forType: type) {
        result[type.rawValue] = data.base64EncodedString()
      }
    }
  }

  let payload: [String: Any] = ["items": payloadItems]
  do {
    let jsonData = try JSONSerialization.data(withJSONObject: payload, options: [])
    return NativeClipboardSnapshotResponse(
      payload: jsonData.base64EncodedString().intoRustString(),
      error_message: nil,
      recovery_hint: nil
    )
  } catch {
    return NativeClipboardSnapshotResponse(
      payload: nil,
      error_message: "failed to encode clipboard snapshot: \(error)".intoRustString(),
      recovery_hint: "retry clipboard capture".intoRustString()
    )
  }
}

func restore_clipboard(snapshot_payload: RustString) -> NativeActionResponse {
  let payload = snapshot_payload.toString()
  guard let payloadData = Data(base64Encoded: payload) else {
    return nativeActionError("invalid clipboard payload base64", "discard the stale clipboard snapshot")
  }

  let decoded: Any
  do {
    decoded = try JSONSerialization.jsonObject(with: payloadData)
  } catch {
    return nativeActionError("invalid clipboard payload json: \(error)", "discard the stale clipboard snapshot")
  }

  guard let payloadDictionary = decoded as? [String: Any],
        let payloadItems = payloadDictionary["items"] as? [[String: String]] else {
    return nativeActionError("clipboard payload is missing items", "discard the stale clipboard snapshot")
  }

  let pasteboard = NSPasteboard.general
  pasteboard.clearContents()

  var items = [NSPasteboardItem]()
  for payloadItem in payloadItems {
    let item = NSPasteboardItem()
    for (typeRawValue, encodedData) in payloadItem {
      guard let data = Data(base64Encoded: encodedData) else {
        return nativeActionError(
          "invalid base64 clipboard item for type \(typeRawValue)",
          "discard the stale clipboard snapshot"
        )
      }
      item.setData(data, forType: NSPasteboard.PasteboardType(typeRawValue))
    }
    items.append(item)
  }

  if !items.isEmpty && !pasteboard.writeObjects(items) {
    return nativeActionError("failed to restore clipboard items", "retry clipboard restore")
  }

  return nativeActionOk()
}

func set_clipboard_text(text: RustString) -> NativeActionResponse {
  let pasteboard = NSPasteboard.general
  pasteboard.clearContents()
  if pasteboard.setString(text.toString(), forType: .string) {
    return nativeActionOk()
  }
  return nativeActionError("failed to set clipboard text", "retry clipboard write")
}
