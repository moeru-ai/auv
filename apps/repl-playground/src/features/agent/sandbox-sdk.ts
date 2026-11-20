import { AuvConnection, AuvRpcError, createRunnerClient } from '@auv-js/sdk'
import { base64Decode, base64Encode } from '@bufbuild/protobuf/wire'

export {
  above,
  ApplicationActivationMethod,
  ApplicationIdKind,
  at,
  below,
  BuiltInCursor,
  CaptureResolution,
  center,
  contains,
  DisturbanceLevel,
  Easing,
  ImageEncoding,
  InputDeliveryPath,
  InputPolicy,
  inset,
  intersect,
  leftOf,
  MouseButton,
  offset,
  PermissionStatus,
  Position,
  region,
  rightOf,
  SCROLL_UNTIL_DEFAULTS,
  ScrollDeliveryCandidate,
  ScrollStreamStopReason,
  ScrollUntilStopReason,
  StandardMotionTimingFunction,
  TextSubmit,
  WindowClickStrategy,
  WindowMutationPath,
  WindowStateTarget,
} from '@auv-js/sdk'

declare function runnerClose(id: number): Promise<void>
declare function runnerHalfClose(id: number): Promise<void>
declare function runnerNext(id: number): Promise<null | string>
declare function runnerOpen(method: string): Promise<number>
declare function runnerSend(request: { body: string, id: number }): Promise<void>
declare function runnerUnary(request: { body: string, jsonBody: string, method: string }): Promise<{ body: string } | { json: string }>

async function rpc<T>(request: Promise<T>): Promise<T> {
  try {
    return await request
  }
  catch (error) {
    if (error instanceof Error && 'rpcCode' in error && typeof error.rpcCode === 'number')
      throw new AuvRpcError(error.rpcCode, error.message)
    throw error
  }
}

export const auv = createRunnerClient(new AuvConnection({
  close() {},
  async connect() {},
  async duplex({ method }) {
    const id = await rpc(runnerOpen(method))
    return {
      close: () => runnerClose(id),
      halfClose: () => rpc(runnerHalfClose(id)),
      responses: (async function* () {
        try {
          for (;;) {
            const body = await rpc(runnerNext(id))
            if (body === null)
              return
            yield base64Decode(body)
          }
        }
        finally {
          await runnerClose(id)
        }
      })(),
      send: body => rpc(runnerSend({ body: base64Encode(body), id })),
    }
  },
  async unary({ body, decodeJson, jsonBody, method }) {
    const result = await rpc(runnerUnary({ body: base64Encode(body), jsonBody, method }))
    return 'json' in result ? decodeJson(result.json) : base64Decode(result.body)
  },
}), { runnerClass: 'auv.core.local' })
