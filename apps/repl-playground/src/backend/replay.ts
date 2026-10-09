import type { DuplexCall, DuplexCallOptions, Transport, UnaryCall } from '@auv-js/sdk'

import type { BridgedError, Exchanged } from '../runtime/sdk-bridge'
import type { Backend, CapturedFrame, RunBackend, RunOutcomeKind, SdkAccess } from './types'

import { bridgedError, remoteError } from '../runtime/sdk-bridge'

/** One SDK call of a live run, as encoded messages. */
export interface RecordedRpc {
  error?: BridgedError
  /** Request and response messages in the order they crossed. */
  exchange: Exchanged[]
  /** A JSON-encoded HTTP binding's response text, in place of a response message. */
  json?: string
  /** gRPC path, `/package.Service/Method`. */
  method: string
}

/** A live run's SDK calls, plus the capture images the canvas loaded during it. */
export interface Recording {
  /**
   * Images by capture reference. NOTICE(replay-capture-images): the canvas
   * loads bitmaps asynchronously after a capture call returns, so its image
   * fetches are kept out of the ordered call log and looked up by reference.
   */
  images: Map<string, Blob>
  label: string
  /** The script's SDK calls; see `ReplayTransport`. */
  rpc: RecordedRpc[]
  /** The live device's SDK access, for decoding and routing those calls offline. */
  sdk: Omit<SdkAccess, 'target'>
}

/**
 * Wraps a live backend for one run and records every SDK call and canvas
 * image, so the same script can later run offline against the recording.
 */
export class RecordingBackend implements RunBackend {
  readonly images = new Map<string, Blob>()
  readonly kind: Backend['kind']
  readonly label: string
  readonly rpc: RecordedRpc[] = []
  readonly #transports = new WeakMap<Transport, Transport>()

  constructor(private readonly inner: Backend) {
    this.kind = inner.kind
    this.label = inner.label
  }

  beginRun(): Promise<string | undefined> {
    return this.inner.beginRun()
  }

  async captureImage(frame: CapturedFrame, maxSize: { height: number, width: number }): Promise<Blob> {
    const image = await this.inner.captureImage(frame, maxSize)
    this.images.set(frame.ref, image)
    return image
  }

  endRun(outcome: RunOutcomeKind): Promise<void> {
    return this.inner.endRun(outcome)
  }

  /** The recording of this run so far. */
  recording(): Recording {
    const { describe, route } = this.inner.sdk()
    return { images: this.images, label: this.label, rpc: this.rpc, sdk: { describe, route } }
  }

  sdk(): SdkAccess {
    const access = this.inner.sdk()
    let recording = this.#transports.get(access.target.transport)
    if (!recording) {
      recording = recordingTransport(access.target.transport, this.rpc)
      this.#transports.set(access.target.transport, recording)
    }
    return { ...access, target: { ...access.target, transport: recording } }
  }
}

/**
 * Answers a run's SDK calls from a recording (`ReplayTransport`). A call the
 * recording does not have raises `ReplayDivergence`, marking where the edited
 * script needs a live device again.
 *
 * TODO(replay-determinism): `Date.now()`, `Math.random()` and `sleep()` are not
 * recorded, so scripts that branch on them may diverge spuriously. Record them
 * in the worker when that shows up in practice.
 */
export class ReplayBackend implements RunBackend {
  readonly kind = 'replay'
  readonly label: string
  readonly #access: SdkAccess

  constructor(private readonly recording: Recording) {
    this.label = `Replay of ${recording.label}`
    this.#access = { ...recording.sdk, target: { transport: new ReplayTransport(recording.rpc) } }
  }

  async beginRun(): Promise<string | undefined> {
    return undefined
  }

  async captureImage(frame: CapturedFrame): Promise<Blob> {
    const image = this.recording.images.get(frame.ref)
    if (!image)
      throw new Error(`The recording has no image for capture ${frame.ref}`)
    return image
  }

  async endRun(): Promise<void> {}

  sdk(): SdkAccess {
    return this.#access
  }
}

export class ReplayDivergence extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'ReplayDivergence'
  }
}

/**
 * Answers scripts' direct SDK calls from a recording. A call matches the first
 * unused recorded call with the same method and request bytes, so calls a
 * script made concurrently replay in any order. Streams replay in lockstep:
 * each recorded request waits for the script's matching request.
 * A call or request the recording does not have raises `ReplayDivergence`.
 */
export class ReplayTransport implements Transport {
  readonly #used = new Set<number>()

  constructor(private readonly calls: readonly RecordedRpc[]) {}

  close(): void {}

  async connect(): Promise<void> {}

  async duplex(call: DuplexCallOptions): Promise<DuplexCall> {
    const sent: Array<Uint8Array | undefined> = []
    let wake: (() => void) | undefined
    const push = (body: Uint8Array | undefined) => {
      sent.push(body)
      wake?.()
    }
    const nextSent = async (): Promise<Uint8Array | undefined> => {
      while (sent.length === 0)
        await new Promise<void>((resolve) => { wake = resolve })
      return sent.shift()
    }
    const take = (method: string, body: Uint8Array) => this.#take(method, body)
    let open = true
    return {
      close: () => {
        open = false
        push(undefined)
      },
      halfClose: async () => push(undefined),
      responses: (async function* () {
        const first = await nextSent()
        if (!first)
          return
        const entry = take(call.method, first)
        for (const frame of entry.exchange.slice(1)) {
          if ('response' in frame) {
            await pace()
            yield frame.response
            continue
          }
          const body = await nextSent()
          if (!open)
            return
          if (!body || !equalBytes(body, frame.request))
            throw new ReplayDivergence(`${methodName(call.method)} diverged: the script sent a different stream message than the recording.`)
        }
        if (entry.error)
          throw remoteError(entry.error)
      })(),
      send: async body => push(body),
    }
  }

  async unary(call: UnaryCall): Promise<Uint8Array> {
    const entry = this.#take(call.method, call.body)
    await pace()
    if (entry.error)
      throw remoteError(entry.error)
    if (entry.json !== undefined)
      return call.decodeJson(entry.json)
    const response = entry.exchange.find(frame => 'response' in frame)
    if (!response || !('response' in response))
      throw new ReplayDivergence(`${methodName(call.method)} has no recorded response.`)
    return response.response
  }

  #take(method: string, body: Uint8Array): RecordedRpc {
    const index = this.calls.findIndex((entry, candidate) => !this.#used.has(candidate) && entry.method === method && requestOf(entry) !== undefined && equalBytes(requestOf(entry)!, body))
    if (index === -1) {
      const recorded = this.calls.some(entry => entry.method === method)
      throw new ReplayDivergence(recorded
        ? `${methodName(method)} diverged: the recording has no such call with these arguments; run live from here.`
        : `${methodName(method)} goes beyond the recording; run live from here.`)
    }
    this.#used.add(index)
    return this.calls[index]!
  }
}

function equalBytes(a: Uint8Array, b: Uint8Array): boolean {
  return a.byteLength === b.byteLength && a.every((byte, index) => byte === b[index])
}

function methodName(method: string): string {
  return method.slice(method.lastIndexOf('/') + 1)
}

// Keep live-like pacing short so stepping and the time strip stay readable.
async function pace(): Promise<void> {
  await new Promise(resolve => setTimeout(resolve, 4))
}

/** Wraps a live transport so every call is appended to `log` as it ends. */
function recordingTransport(inner: Transport, log: RecordedRpc[]): Transport {
  return {
    close: options => inner.close(options),
    connect: options => inner.connect(options),
    async duplex(call) {
      const entry: RecordedRpc = { exchange: [], method: call.method }
      log.push(entry)
      const stream = await inner.duplex(call)
      return {
        close: options => stream.close(options),
        halfClose: () => stream.halfClose(),
        responses: (async function* () {
          try {
            for await (const body of stream.responses) {
              entry.exchange.push({ response: body })
              yield body
            }
          }
          catch (error) {
            entry.error = bridgedError(error)
            throw error
          }
        })(),
        send: async (body) => {
          entry.exchange.push({ request: body })
          await stream.send(body)
        },
      }
    },
    async unary(call) {
      const entry: RecordedRpc = { exchange: [{ request: call.body }], method: call.method }
      log.push(entry)
      try {
        const body = await inner.unary({
          ...call,
          decodeJson: (text) => {
            entry.json = text
            return call.decodeJson(text)
          },
        })
        if (entry.json === undefined)
          entry.exchange.push({ response: body })
        return body
      }
      catch (error) {
        entry.error = bridgedError(error)
        throw error
      }
    },
  }
}

function requestOf(entry: RecordedRpc): Uint8Array | undefined {
  const first = entry.exchange[0]
  return first && 'request' in first ? first.request : undefined
}
