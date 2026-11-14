import type { WireValue } from '../runtime/protocol'

/** Structured-clone-safe summary of a script value; keeps protobuf `$typeName`s for lineage. */
export function toWire(value: unknown, depth = 0, seen = new WeakSet<object>()): WireValue {
  if (value === null || value === undefined)
    return value
  switch (typeof value) {
    case 'bigint':
      return `${value}n`
    case 'boolean':
    case 'number':
    case 'string':
      return value
    case 'function':
      return { $fn: value.name || 'anonymous' }
    case 'symbol':
      return value.toString()
  }
  const object = value as object
  if (seen.has(object))
    return { $circular: true }
  if (depth > 6)
    return { $truncated: true }
  seen.add(object)
  if (object instanceof Error)
    return { $error: `${object.name}: ${object.message}` }
  if (ArrayBuffer.isView(object))
    return { $bytes: object.byteLength }
  if (object instanceof Map)
    return { $map: [...object.entries()].slice(0, 200).map(([k, v]) => [toWire(k, depth + 1, seen), toWire(v, depth + 1, seen)]) }
  if (object instanceof Set)
    return { $set: [...object].slice(0, 200).map(v => toWire(v, depth + 1, seen)) }
  if (Array.isArray(object))
    return object.slice(0, 500).map(item => toWire(item, depth + 1, seen))
  const out: Record<string, WireValue> = {}
  for (const [key, item] of Object.entries(object))
    out[key] = toWire(item, depth + 1, seen)
  return out
}
