import type { InputActionFailure } from '../../gen/auv/api/driver/v1/input_pb'

import { anyUnpack } from '@bufbuild/protobuf/wkt'

import { DisturbanceLevel, InputActionFailureSchema, InputDeliveryPath } from '../../gen/auv/api/driver/v1/input_pb'
import { AuvRpcError } from '../../transport/errors'
import { decodeRpcStatus } from '../../transport/rpc-status'

/** Known progress of one failed native action; absent progress is unknown. */
export type InputActionProgress = Exclude<InputActionFailure['progress'], { case: undefined }>

/** Reads a failed RPC's progress without changing its original error/category. */
export function inputActionProgress(error: unknown): InputActionProgress | undefined {
  if (!(error instanceof AuvRpcError) || error.details === undefined || error.rpcCode === 0)
    return undefined
  try {
    const status = decodeRpcStatus(error.details)
    if (status === undefined || status.code !== error.rpcCode || status.message !== error.message)
      return undefined
    const typeUrl = `type.googleapis.com/${InputActionFailureSchema.typeName}`
    const details = status.details.filter(detail => detail.typeUrl === typeUrl)
    if (details.length !== 1)
      return undefined
    const failure = anyUnpack(details[0]!, InputActionFailureSchema)
    const progress = failure?.progress
    if (progress === undefined || progress.case === undefined)
      return undefined
    if (progress.case === 'notDelivered')
      return progress
    const action = progress.value
    const paths = Object.values(InputDeliveryPath)
    const knownPath = (path: InputDeliveryPath) => path !== InputDeliveryPath.UNSPECIFIED && paths.includes(path)
    if (action.verified || !knownPath(action.selectedPath)
      || action.selectedPath === InputDeliveryPath.NOOP || action.selectedPath === InputDeliveryPath.UNSUPPORTED) {
      return undefined
    }
    const disturbances = [action.mouseDisturbance, action.focusDisturbance, action.clipboardDisturbance]
    if (disturbances.some(level => level === DisturbanceLevel.UNSPECIFIED || !Object.values(DisturbanceLevel).includes(level)))
      return undefined
    if (action.attempts.some(attempt => !knownPath(attempt.path) || (attempt.succeeded && attempt.path !== action.selectedPath)))
      return undefined
    const last = action.attempts.at(-1)
    if (last === undefined || last.path !== action.selectedPath || last.succeeded !== (progress.case === 'submitted'))
      return undefined
    return progress
  }
  catch {
    // An invalid or future receipt cannot establish that no input occurred.
    return undefined
  }
}
