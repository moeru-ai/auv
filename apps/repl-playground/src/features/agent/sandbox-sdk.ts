import { AuvConnection, createRunnerClient } from '@auv-js/sdk'
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

export const auv = createRunnerClient(new AuvConnection({
  close() {},
  async connect() {},
  async duplex({ method }) {
    const id = await runnerOpen(method)
    return {
      close: () => runnerClose(id),
      halfClose: () => runnerHalfClose(id),
      responses: (async function* () {
        try {
          for (;;) {
            const body = await runnerNext(id)
            if (body === null)
              return
            yield base64Decode(body)
          }
        }
        finally {
          await runnerClose(id)
        }
      })(),
      send: body => runnerSend({ body: base64Encode(body), id }),
    }
  },
  async unary({ body, decodeJson, jsonBody, method }) {
    const result = await runnerUnary({ body: base64Encode(body), jsonBody, method })
    return 'json' in result ? decodeJson(result.json) : base64Decode(result.body)
  },
}), { runnerClass: 'auv.core.local' })
