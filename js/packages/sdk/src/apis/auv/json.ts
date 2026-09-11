import type { DescEnum, DescField, DescMessage } from '@bufbuild/protobuf'

import { ScalarType } from '@bufbuild/protobuf'
import { FeatureSet_FieldPresence } from '@bufbuild/protobuf/wkt'

export type ProtobufJsonSchema = Readonly<Record<string, unknown>>

// TODO(protobuf-json-schema-conformance): Separate parser-accepted input from
// canonical output and cover the remaining ProtoJSON edge cases after an
// owner-approved conformance slice; see
// `docs/ai/references/session-api/2026-08-17-protobuf-json-schema-library-research.md`.
/** Projects one Protobuf request or response message into its ProtoJSON schema. */
export function protobufJsonSchema(message: DescMessage): ProtobufJsonSchema {
  const definitions: Record<string, Record<string, unknown>> = {}
  const building = new Set<string>()
  let schemaForField: (field: DescField) => Record<string, unknown>

  const defineMessage = (current: DescMessage): Record<string, unknown> => {
    const existing = definitions[current.typeName]
    if (existing !== undefined)
      return existing

    const definition: Record<string, unknown> = {}
    definitions[current.typeName] = definition
    if (building.has(current.typeName))
      return definition

    building.add(current.typeName)
    Object.assign(definition, wellKnownMessageSchema(current) ?? objectSchema(current, schemaForField, defineMessage))
    building.delete(current.typeName)
    return definition
  }

  const schemaForMessage = (current: DescMessage): Record<string, unknown> => {
    const wellKnown = wellKnownMessageSchema(current)
    if (wellKnown !== undefined)
      return wellKnown

    defineMessage(current)
    return { $ref: `#/$defs/${jsonPointerSegment(current.typeName)}` }
  }

  schemaForField = (field: DescField): Record<string, unknown> => {
    switch (field.fieldKind) {
      case 'enum':
        return enumSchema(field.enum)
      case 'list': {
        const items = field.listKind === 'message'
          ? schemaForMessage(field.message)
          : field.listKind === 'enum'
            ? enumSchema(field.enum)
            : scalarSchema(field.scalar)
        return { items, type: 'array' }
      }
      case 'map': {
        const additionalProperties = field.mapKind === 'message'
          ? schemaForMessage(field.message)
          : field.mapKind === 'enum'
            ? enumSchema(field.enum)
            : scalarSchema(field.scalar)
        return { additionalProperties, type: 'object' }
      }
      case 'message':
        return schemaForMessage(field.message)
      case 'scalar':
        return scalarSchema(field.scalar)
    }
  }

  // Rebuild after the field projector exists. The early closure above keeps
  // recursive message references behind shared $defs instead of recursing.
  delete definitions[message.typeName]
  defineMessage(message)

  return {
    $defs: definitions,
    $ref: `#/$defs/${jsonPointerSegment(message.typeName)}`,
    $schema: 'https://json-schema.org/draft/2020-12/schema',
  }
}

function enumSchema(value: DescEnum): Record<string, unknown> {
  return {
    enum: value.values.map(item => item.name),
    type: 'string',
  }
}

function floatingPointSchema(): Record<string, unknown> {
  // ProtoJSON spells non-finite values as strings.
  return {
    anyOf: [
      { type: 'number' },
      { enum: ['NaN', 'Infinity', '-Infinity'], type: 'string' },
    ],
  }
}

function jsonPointerSegment(value: string): string {
  return value.replaceAll('~', '~0').replaceAll('/', '~1')
}

function objectSchema(
  message: DescMessage,
  fieldSchema: (field: DescField) => Record<string, unknown>,
  defineMessage: (message: DescMessage) => Record<string, unknown>,
): Record<string, unknown> {
  // Ensure recursive dependencies have placeholders before their fields are
  // visited. The returned schemas reference those shared definitions.
  for (const field of message.fields) {
    if (field.fieldKind === 'message')
      defineMessage(field.message)
    else if (field.fieldKind === 'list' && field.listKind === 'message')
      defineMessage(field.message)
    else if (field.fieldKind === 'map' && field.mapKind === 'message')
      defineMessage(field.message)
  }

  const properties = Object.fromEntries(message.fields.map(field => [field.jsonName, fieldSchema(field)]))
  // TODO(protobuf-tool-validation): Proto3 presence does not express required
  // business inputs. Add those constraints after AUV owns a field-validation
  // annotation instead of guessing from scalar defaults.
  const required = message.fields
    .filter(field => field.presence === FeatureSet_FieldPresence.LEGACY_REQUIRED)
    .map(field => field.jsonName)
  const oneofs = message.oneofs.map((oneof) => {
    const alternatives = oneof.fields.map(field => ({ required: [field.jsonName] }))
    return {
      oneOf: [
        ...alternatives,
        { not: { anyOf: alternatives } },
      ],
    }
  })

  return {
    additionalProperties: false,
    ...(oneofs.length === 0 ? {} : { allOf: oneofs }),
    properties,
    ...(required.length === 0 ? {} : { required }),
    type: 'object',
  }
}

function scalarSchema(value: ScalarType): Record<string, unknown> {
  switch (value) {
    case ScalarType.BOOL:
      return { type: 'boolean' }
    case ScalarType.BYTES:
      return { contentEncoding: 'base64', type: 'string' }
    case ScalarType.DOUBLE:
      return floatingPointSchema()
    case ScalarType.FIXED32:
      return { maximum: 4_294_967_295, minimum: 0, type: 'integer' }
    case ScalarType.FIXED64:
      return { pattern: '^[0-9]+$', type: 'string' }
    case ScalarType.FLOAT:
      return floatingPointSchema()
    case ScalarType.INT32:
      return { type: 'integer' }
    case ScalarType.INT64:
      // ProtoJSON represents every 64-bit integer as a decimal string.
      return { pattern: '^-?[0-9]+$', type: 'string' }
    case ScalarType.SFIXED32:
      return { type: 'integer' }
    case ScalarType.SFIXED64:
      return { pattern: '^-?[0-9]+$', type: 'string' }
    case ScalarType.SINT32:
      return { type: 'integer' }
    case ScalarType.SINT64:
      return { pattern: '^-?[0-9]+$', type: 'string' }
    case ScalarType.STRING:
      return { type: 'string' }
    case ScalarType.UINT32:
      return { maximum: 4_294_967_295, minimum: 0, type: 'integer' }
    case ScalarType.UINT64:
      return { pattern: '^[0-9]+$', type: 'string' }
  }
}

function wellKnownMessageSchema(message: DescMessage): Record<string, unknown> | undefined {
  switch (message.typeName) {
    case 'google.protobuf.Any':
      return { additionalProperties: true, type: 'object' }
    case 'google.protobuf.BoolValue':
      return { type: 'boolean' }
    case 'google.protobuf.BytesValue':
      return { contentEncoding: 'base64', type: 'string' }
    case 'google.protobuf.DoubleValue':
      return floatingPointSchema()
    case 'google.protobuf.Duration':
      return { pattern: '^-?[0-9]+(?:\\.[0-9]{1,9})?s$', type: 'string' }
    case 'google.protobuf.Empty':
      return { additionalProperties: false, properties: {}, type: 'object' }
    case 'google.protobuf.FieldMask':
      return { type: 'string' }
    case 'google.protobuf.FloatValue':
      return floatingPointSchema()
    case 'google.protobuf.Int32Value':
      return { type: 'integer' }
    case 'google.protobuf.Int64Value':
      return { pattern: '^-?[0-9]+$', type: 'string' }
    case 'google.protobuf.ListValue':
      return { items: {}, type: 'array' }
    case 'google.protobuf.StringValue':
      return { type: 'string' }
    case 'google.protobuf.Struct':
      return { additionalProperties: true, type: 'object' }
    case 'google.protobuf.Timestamp':
      return { format: 'date-time', type: 'string' }
    case 'google.protobuf.UInt32Value':
      return { maximum: 4_294_967_295, minimum: 0, type: 'integer' }
    case 'google.protobuf.UInt64Value':
      return { pattern: '^[0-9]+$', type: 'string' }
    case 'google.protobuf.Value':
      return {}
  }
}
