/** An operation was cancelled through its AbortSignal. */
export class AuvAbortError extends Error {
  constructor(reason: unknown) {
    super('AUV operation was aborted', { cause: reason })
    this.name = 'AuvAbortError'
  }
}

/** Caller options describe conflicting execution placement. */
export class AuvConfigurationError extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'AuvConfigurationError'
  }
}

/** The daemon or Runner returned a non-success operation result. */
export class AuvRemoteError extends Error {
  constructor(message: string, cause?: unknown) {
    super(message, { cause })
    this.name = 'AuvRemoteError'
  }
}

/** RFC 9457 problem returned by an AUV HTTP endpoint. */
export class AuvHttpError extends AuvRemoteError {
  readonly status: number
  readonly type: string

  constructor(problem: { detail: string, status: number, title: string, type: string }) {
    super(problem.detail, problem)
    this.name = 'AuvHttpError'
    this.status = problem.status
    this.type = problem.type
  }
}

/** A response violated the AUV wire contract. */
export class AuvProtocolError extends Error {
  constructor(message: string, cause?: unknown) {
    super(message, { cause })
    this.name = 'AuvProtocolError'
  }
}

/** A routed RPC completed with a non-success status. */
export class AuvRpcError extends AuvRemoteError {
  constructor(readonly rpcCode: number, message: string, cause?: unknown) {
    super(message, cause)
    this.name = 'AuvRpcError'
  }
}

/** A connection or transport failed before returning an AUV response. */
export class AuvTransportError extends Error {
  constructor(message: string, cause?: unknown) {
    super(message, { cause })
    this.name = 'AuvTransportError'
  }
}

export function abortError(signal: AbortSignal): AuvAbortError {
  return new AuvAbortError(signal.reason)
}

export function auvHttpError(response: Response, error: unknown): AuvHttpError {
  if (isProblem(error)) {
    return new AuvHttpError(error)
  }
  return new AuvHttpError({
    detail: typeof error === 'string' ? error : response.statusText,
    status: response.status,
    title: response.statusText,
    type: 'about:blank',
  })
}

export function throwIfAborted(signal?: AbortSignal): void {
  if (signal?.aborted)
    throw abortError(signal)
}

function isProblem(value: unknown): value is { detail: string, status: number, title: string, type: string } {
  return typeof value === 'object'
    && value !== null
    && typeof Reflect.get(value, 'detail') === 'string'
    && typeof Reflect.get(value, 'status') === 'number'
    && typeof Reflect.get(value, 'title') === 'string'
    && typeof Reflect.get(value, 'type') === 'string'
}
