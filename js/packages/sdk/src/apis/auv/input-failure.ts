import type { DescMessage, MessageShape } from '@bufbuild/protobuf'

import type { InputActionFailure, InputActionResult, KeyboardInputProgress } from '../../gen/auv/api/driver/v1/input_pb'

import { anyUnpack } from '@bufbuild/protobuf/wkt'

import { DisturbanceLevel, InputActionFailureSchema, InputDeliveryPath, InputFailureReason, KeyboardInputProgressSchema } from '../../gen/auv/api/driver/v1/input_pb'
import { AuvRpcError } from '../../transport/errors'
import { decodeRpcStatus } from '../../transport/rpc-status'

/** Known progress of one failed native action; absent progress is unknown. */
export type InputActionProgress = Exclude<InputActionFailure['progress'], { case: undefined }>

/** Reads a failed RPC's progress without changing its original error/category. */
export function inputActionProgress(error: unknown): InputActionProgress | undefined {
  const progress = inputFailure(error)?.progress
  if (progress === undefined || progress.case === undefined)
    return undefined
  if (progress.case === 'notDelivered')
    return progress
  const action = progress.value
  if (!validAction(action) || action.verified
    || action.selectedPath === InputDeliveryPath.NOOP || action.selectedPath === InputDeliveryPath.UNSUPPORTED) {
    return undefined
  }
  const last = action.attempts.at(-1)
  if (last === undefined || last.path !== action.selectedPath || last.succeeded !== (progress.case === 'submitted'))
    return undefined
  return progress
}

/** Reads a known failure reason independently of whether delivery is known. */
export function inputFailureReason(error: unknown): InputFailureReason | undefined {
  const reason = inputFailure(error)?.reason
  return reason === InputFailureReason.TARGET_LOST ? reason : undefined
}

/** Completed actions and repetitions do not establish delivery of the failed action. */
export function keyboardInputProgress(error: unknown): KeyboardInputProgress | undefined {
  const progress = inputDetail(error, KeyboardInputProgressSchema)
  if (progress?.actionIndex === undefined || !progress.completed.every(validAction))
    return undefined
  return progress
}

// Native and batch receipts share strict framing, but retain separate meanings.
function inputDetail<Schema extends DescMessage>(error: unknown, schema: Schema): MessageShape<Schema> | undefined {
  if (!(error instanceof AuvRpcError) || error.details === undefined || error.rpcCode === 0)
    return undefined
  try {
    const status = decodeRpcStatus(error.details)
    if (status === undefined || status.code !== error.rpcCode || status.message !== error.message)
      return undefined
    const typeUrl = `type.googleapis.com/${schema.typeName}`
    const details = status.details.filter(detail => detail.typeUrl === typeUrl)
    if (details.length !== 1)
      return undefined
    return anyUnpack(details[0]!, schema)
  }
  catch {
    // An invalid or future receipt cannot establish that no input occurred.
    return undefined
  }
}

function inputFailure(error: unknown): InputActionFailure | undefined {
  const failure = inputDetail(error, InputActionFailureSchema)
  if (failure === undefined)
    return undefined
  const targetLost = failure.reason === InputFailureReason.TARGET_LOST
  if (targetLost && (!(error instanceof AuvRpcError) || error.rpcCode !== 9))
    return undefined
  if (failure.progress.case === undefined && !targetLost)
    return undefined
  return failure
}

function validAction(action: InputActionResult): boolean {
  const paths = Object.values(InputDeliveryPath)
  const knownPath = (path: InputDeliveryPath) => path !== InputDeliveryPath.UNSPECIFIED && paths.includes(path)
  if (!knownPath(action.selectedPath))
    return false
  const disturbances = [action.mouseDisturbance, action.focusDisturbance, action.clipboardDisturbance]
  if (disturbances.some(level => level === DisturbanceLevel.UNSPECIFIED || !Object.values(DisturbanceLevel).includes(level)))
    return false
  return action.attempts.every(attempt => knownPath(attempt.path) && (!attempt.succeeded || attempt.path === action.selectedPath))
}
