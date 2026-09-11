import type { Duration, Timestamp } from '@bufbuild/protobuf/wkt'

import { create } from '@bufbuild/protobuf'
import { DurationSchema } from '@bufbuild/protobuf/wkt'
import { milliseconds } from 'date-fns'

import { AuvProtocolError } from '../../transport/errors'

export function duration(milliseconds: number): Duration {
  return create(DurationSchema, {
    nanos: Math.trunc((milliseconds % 1_000) * 1_000_000),
    seconds: BigInt(Math.trunc(milliseconds / 1_000)),
  })
}

export function durationMilliseconds(value: Duration | undefined): number | undefined {
  if (typeof value === 'undefined') {
    return
  }

  return milliseconds({ seconds: Number(value.seconds) }) + value.nanos / 1_000_000
}

export function timestampDate(value: Timestamp | undefined): Date | undefined {
  return value === undefined
    ? undefined
    : new Date(Number(value.seconds) * 1_000 + value.nanos / 1_000_000)
}

export function unknownEnum(field: string, value: never): never {
  throw new AuvProtocolError(`AUV response returned unknown ${field} value ${value}`)
}
