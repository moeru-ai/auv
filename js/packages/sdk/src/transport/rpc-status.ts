import type { Any } from '@bufbuild/protobuf/wkt'

import { fromBinary } from '@bufbuild/protobuf'
import { BinaryReader, WireType } from '@bufbuild/protobuf/wire'
import { AnySchema } from '@bufbuild/protobuf/wkt'

/** Decodes the standard rich-error envelope; malformed bytes stay unknown. */
export function decodeRpcStatus(bytes: Uint8Array) {
  // NOTICE(device-entry-status-decode): the SDK does not generate
  // google.rpc.Status. Decode its code/message/details fields here so Driver
  // input failures and the existing Device-entry reader share this wire rule.
  try {
    const reader = new BinaryReader(bytes)
    let code = 0
    let message = ''
    const details: Any[] = []
    while (reader.pos < reader.len) {
      const [field, wireType] = reader.tag()
      if (field === 1 && wireType === WireType.Varint)
        code = reader.int32()
      else if (field === 2 && wireType === WireType.LengthDelimited)
        message = reader.string()
      else if (field === 3 && wireType === WireType.LengthDelimited)
        details.push(fromBinary(AnySchema, reader.bytes()))
      else reader.skip(wireType)
    }
    return { code, details, message }
  }
  catch {
    return undefined
  }
}
