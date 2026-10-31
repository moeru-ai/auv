import type { MessageInitShape } from '@bufbuild/protobuf'
import type { Any } from '@bufbuild/protobuf/wkt'

import { create, toBinary } from '@bufbuild/protobuf'
import { BinaryWriter, WireType } from '@bufbuild/protobuf/wire'
import { anyPack, AnySchema } from '@bufbuild/protobuf/wkt'
import { describe, expect, it } from 'vitest'

import { DisturbanceLevel, InputActionFailureSchema, InputActionResultSchema, InputDeliveryPath, InputFailureReason, KeyboardInputProgressSchema } from '../../gen/auv/api/driver/v1/input_pb'
import { AuvConnection } from '../../transport/connection'
import { AuvRpcError } from '../../transport/errors'
import { createAuv } from './client'
import { inputActionProgress, inputFailureReason, keyboardInputProgress } from './input-failure'

function receipt(progress: MessageInitShape<typeof InputActionFailureSchema>['progress'], reason = InputFailureReason.UNSPECIFIED): Any {
  return anyPack(InputActionFailureSchema, create(InputActionFailureSchema, { progress, reason }))
}

function status(details: Any[], code = 7): Uint8Array {
  const writer = new BinaryWriter().tag(1, WireType.Varint).int32(code).tag(2, WireType.LengthDelimited).string('UIA denied')
  for (const detail of details)
    writer.tag(3, WireType.LengthDelimited).bytes(toBinary(AnySchema, detail))
  return writer.finish()
}

function submittedAction() {
  return create(InputActionResultSchema, {
    attempts: [{ path: InputDeliveryPath.AX_FOCUS, succeeded: true }],
    clipboardDisturbance: DisturbanceLevel.NONE,
    focusDisturbance: DisturbanceLevel.FOREGROUND,
    mouseDisturbance: DisturbanceLevel.NONE,
    selectedPath: InputDeliveryPath.AX_FOCUS,
    verified: false,
  })
}

describe('failed input progress', () => {
  it('preserves target loss when native delivery remains unknown', () => {
    const error = new AuvRpcError(9, 'UIA denied', undefined, status([receipt(undefined, InputFailureReason.TARGET_LOST)], 9))
    expect(inputFailureReason(error)).toBe(InputFailureReason.TARGET_LOST)
    expect(inputActionProgress(error)).toBeUndefined()
    expect(keyboardInputProgress(error)).toBeUndefined()
    const conflicting = new AuvRpcError(7, 'UIA denied', undefined, status([receipt(undefined, InputFailureReason.TARGET_LOST)]))
    expect(inputFailureReason(conflicting)).toBeUndefined()
    expect(inputActionProgress(conflicting)).toBeUndefined()
  })

  it('keeps batch counts, submitted prefix and target loss in one standard status', () => {
    const batch = create(KeyboardInputProgressSchema, { actionIndex: 1, completed: [submittedAction()], completedPresses: 1 })
    const error = new AuvRpcError(9, 'UIA denied', undefined, status([
      anyPack(KeyboardInputProgressSchema, batch),
      receipt({ case: 'submitted', value: submittedAction() }, InputFailureReason.TARGET_LOST),
    ], 9))
    expect(inputFailureReason(error)).toBe(InputFailureReason.TARGET_LOST)
    expect(inputActionProgress(error)?.case).toBe('submitted')
    expect(keyboardInputProgress(error)?.actionIndex).toBe(1)
    expect(keyboardInputProgress(error)?.completed).toHaveLength(1)
    expect(keyboardInputProgress(error)?.completedPresses).toBe(1)
  })

  it('leaves legacy raw batch details and duplicate batch receipts unknown', () => {
    const batch = create(KeyboardInputProgressSchema, { actionIndex: 0 })
    expect(keyboardInputProgress(new AuvRpcError(9, 'UIA denied', undefined, toBinary(KeyboardInputProgressSchema, batch)))).toBeUndefined()
    const detail = anyPack(KeyboardInputProgressSchema, batch)
    expect(keyboardInputProgress(new AuvRpcError(9, 'UIA denied', undefined, status([detail, detail], 9)))).toBeUndefined()
  })

  it('preserves submitted unverified input without changing the original error', () => {
    const error = new AuvRpcError(7, 'UIA denied', undefined, status([receipt({ case: 'submitted', value: submittedAction() })]))
    const progress = inputActionProgress(error)
    expect(progress?.case).toBe('submitted')
    if (progress?.case !== 'submitted')
      throw new Error('submitted receipt missing')
    expect(progress.value.verified).toBe(false)
    expect(progress.value.attempts).toHaveLength(1)
    expect(progress.value.attempts[0]?.succeeded).toBe(true)
    expect(error.rpcCode).toBe(7)
    expect(error.message).toBe('UIA denied')
  })

  it('keeps an uncertain native call distinct from explicit zero delivery', () => {
    const action = submittedAction()
    action.attempts[0]!.succeeded = false
    const uncertain = new AuvRpcError(7, 'UIA denied', undefined, status([receipt({ case: 'deliveryUncertain', value: action })]))
    const refused = new AuvRpcError(7, 'UIA denied', undefined, status([receipt({ case: 'notDelivered', value: {} })]))
    expect(inputActionProgress(uncertain)?.case).toBe('deliveryUncertain')
    expect(inputActionProgress(refused)?.case).toBe('notDelivered')
  })

  it('leaves legacy, conflicting, duplicate, and invalid receipts unknown', () => {
    const detail = receipt({ case: 'notDelivered', value: {} })
    const invalidAction = submittedAction()
    invalidAction.verified = true
    for (const details of [
      undefined,
      new Uint8Array([255]),
      status([detail], 14),
      status([detail, detail]),
      status([receipt(undefined)]),
      status([receipt({ case: 'submitted', value: invalidAction })]),
      status([receipt({ case: 'submitted', value: {} })]),
      status([create(AnySchema, { typeUrl: 'type.googleapis.com/future.InputFailure', value: new Uint8Array([0x0A, 0]) })]),
    ]) {
      const error = new AuvRpcError(7, 'UIA denied', undefined, details)
      expect(inputActionProgress(error)).toBeUndefined()
      expect(error.rpcCode).toBe(7)
    }
  })

  it('retains progress through the public Runner call with one RPC and no fallback', async () => {
    const error = new AuvRpcError(7, 'UIA denied', undefined, status([receipt({ case: 'submitted', value: submittedAction() })]))
    let calls = 0
    const connection = new AuvConnection({
      close() {},
      async connect() {},
      async duplex() { throw new Error('unexpected duplex') },
      async unary() {
        calls++
        throw error
      },
    })
    const runner = createAuv(connection).runner({ runnerClass: 'auv.core.local' })
    await expect(runner.accessibility.focusText({ application: 'fixture.exe', selector: { case: 'query', value: 'Search' } })).rejects.toBe(error)
    expect(inputActionProgress(error)?.case).toBe('submitted')
    expect(calls).toBe(1)
  })
})
