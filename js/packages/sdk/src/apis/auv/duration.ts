import type { DescField, DescMessage, MessageInitShape } from '@bufbuild/protobuf'
import type { Duration } from '@bufbuild/protobuf/wkt'

const DURATION_TYPE_NAME = 'google.protobuf.Duration'

/**
 * A message init in which every `google.protobuf.Duration` may also be a
 * number of milliseconds, such as `settle: 400` or
 * `click: { count: 2, interval: 80 }`. Runner client calls accept this shape
 * and send the numbers as protobuf Durations.
 */
export type WithMillis<T> = [Extract<T, { readonly $typeName: typeof DURATION_TYPE_NAME }>] extends [never]
  ? MillisFields<T>
  : number | T

// A protobuf-es init is a union of the message and its plain init object, so a
// Duration field is found by its message variant, then each member is mapped.
type MillisFields<T>
  = T extends Uint8Array
    ? T
    : T extends readonly (infer Item)[]
      ? WithMillis<Item>[]
      : T extends object
        ? { [K in keyof T]: WithMillis<T[K]> }
        : T

/**
 * Returns `init` with every millisecond number in a Duration field replaced by
 * a protobuf Duration. It walks the message schema, so nested messages, oneof
 * values, lists and maps are covered; other values are kept as they are.
 */
export function durationsFromMillis<Desc extends DescMessage>(schema: Desc, init: WithMillis<MessageInitShape<Desc>>): MessageInitShape<Desc> {
  return messageFromMillis(schema, init) as MessageInitShape<Desc>
}

/** A protobuf Duration for `ms` milliseconds; seconds and nanos share its sign. */
function durationFromMillis(ms: number): Pick<Duration, 'nanos' | 'seconds'> {
  if (!Number.isFinite(ms))
    throw new TypeError(`A Duration in milliseconds must be finite, got ${ms}`)
  const seconds = Math.trunc(ms / 1000)
  return { nanos: Math.round((ms - seconds * 1000) * 1_000_000), seconds: BigInt(seconds) }
}

function fieldFromMillis(field: DescField, value: unknown): unknown {
  switch (field.fieldKind) {
    case 'list':
      return field.listKind === 'message' && Array.isArray(value)
        ? value.map(item => valueFromMillis(field.message, item))
        : value
    case 'map':
      return field.mapKind === 'message' && typeof value === 'object' && value !== null
        ? Object.fromEntries(Object.entries(value).map(([key, item]) => [key, valueFromMillis(field.message, item)]))
        : value
    case 'message':
      return valueFromMillis(field.message, value)
    default:
      return value
  }
}

function messageFromMillis(schema: DescMessage, init: unknown): unknown {
  if (typeof init !== 'object' || init === null)
    return init
  const source = init as Record<string, unknown>
  let result: Record<string, unknown> | undefined
  const set = (key: string, value: unknown) => {
    if (value === source[key])
      return
    result ??= { ...source }
    result[key] = value
  }
  for (const member of schema.members) {
    const value = source[member.localName]
    if (value === undefined)
      continue
    if (member.kind === 'field') {
      set(member.localName, fieldFromMillis(member, value))
      continue
    }
    const selected = value as { case?: string, value?: unknown }
    const field = member.fields.find(candidate => candidate.localName === selected.case)
    if (field) {
      const converted = fieldFromMillis(field, selected.value)
      if (converted !== selected.value)
        set(member.localName, { ...selected, value: converted })
    }
  }
  return result ?? init
}

function valueFromMillis(schema: DescMessage, value: unknown): unknown {
  if (schema.typeName === DURATION_TYPE_NAME && typeof value === 'number')
    return durationFromMillis(value)
  return messageFromMillis(schema, value)
}
