import { fromBinary, toBinary, type DescMessage, type MessageShape } from '@bufbuild/protobuf'

import * as Device from './gen/auv/api/core/v1/device_pb.js'
import * as Discovery from './gen/auv/api/core/v1/discovery_pb.js'
import * as Run from './gen/auv/api/core/v1/run_pb.js'
import * as Runner from './gen/auv/api/core/v1/runner_pb.js'

export interface AuvRestClientOptions {
  /** Override fetch to supply Node TLS credentials or test transport policy. */
  fetch?: typeof globalThis.fetch
}

/** Typed protobuf-over-HTTP client for AUV's REST resource routes. */
export class AuvRestClient {
  readonly #baseUrl: URL
  readonly #fetch: typeof globalThis.fetch

  constructor(endpoint: string, options: AuvRestClientOptions = {}) {
    this.#baseUrl = validatedRestUrl(endpoint)
    this.#fetch = options.fetch ?? globalThis.fetch
  }

  listApiNamespaces(): Promise<Discovery.ListApiNamespacesResponse> {
    return this.#request('GET', '/apis', Discovery.ListApiNamespacesResponseSchema)
  }

  getAuvApiNamespace(): Promise<Discovery.GetApiNamespaceResponse> {
    return this.#request('GET', '/apis/auv', Discovery.GetApiNamespaceResponseSchema)
  }

  getAuvApiGroupVersion(group: string, version: string): Promise<Discovery.GetApiGroupVersionResponse> {
    return this.#request(
      'GET',
      `/apis/auv/${encodeURIComponent(group)}/${encodeURIComponent(version)}`,
      Discovery.GetApiGroupVersionResponseSchema,
    )
  }

  listDevices(): Promise<Device.ListDevicesResponse> {
    return this.#request('GET', '/apis/auv/core/v1/devices', Device.ListDevicesResponseSchema)
  }

  getDevice(deviceId: string): Promise<Device.GetDeviceResponse> {
    return this.#request('GET', `/apis/auv/core/v1/devices/${encodeURIComponent(deviceId)}`, Device.GetDeviceResponseSchema)
  }

  createRun(request: Run.CreateRunRequest): Promise<Run.CreateRunResponse> {
    return this.#post('/apis/auv/runtime/v1/runs', Run.CreateRunRequestSchema, request, Run.CreateRunResponseSchema)
  }

  listRuns(): Promise<Run.ListRunsResponse> {
    return this.#request('GET', '/apis/auv/runtime/v1/runs', Run.ListRunsResponseSchema)
  }

  getRun(runId: string): Promise<Run.GetRunResponse> {
    return this.#request('GET', `/apis/auv/runtime/v1/runs/${encodeURIComponent(runId)}`, Run.GetRunResponseSchema)
  }

  stopRun(runId: string, request: Run.StopRunRequest): Promise<Run.StopRunResponse> {
    return this.#post(
      `/apis/auv/runtime/v1/runs/${encodeURIComponent(runId)}/stop`,
      Run.StopRunRequestSchema,
      request,
      Run.StopRunResponseSchema,
    )
  }

  claimRunner(runId: string, request: Runner.ClaimRunnerRequest): Promise<Runner.ClaimRunnerResponse> {
    return this.#post(
      `/apis/auv/runtime/v1/runs/${encodeURIComponent(runId)}/runnerleases`,
      Runner.ClaimRunnerRequestSchema,
      request,
      Runner.ClaimRunnerResponseSchema,
    )
  }

  releaseRunnerLease(runId: string, leaseId: string): Promise<Runner.ReleaseRunnerLeaseResponse> {
    return this.#request(
      'DELETE',
      `/apis/auv/runtime/v1/runs/${encodeURIComponent(runId)}/runnerleases/${encodeURIComponent(leaseId)}`,
      Runner.ReleaseRunnerLeaseResponseSchema,
    )
  }

  createRunner(request: Runner.CreateRunnerRequest): Promise<Runner.CreateRunnerResponse> {
    return this.#post('/apis/auv/runtime/v1/runners', Runner.CreateRunnerRequestSchema, request, Runner.CreateRunnerResponseSchema)
  }

  listRunners(): Promise<Runner.ListRunnersResponse> {
    return this.#request('GET', '/apis/auv/runtime/v1/runners', Runner.ListRunnersResponseSchema)
  }

  getRunner(runnerId: string): Promise<Runner.GetRunnerResponse> {
    return this.#request('GET', `/apis/auv/runtime/v1/runners/${encodeURIComponent(runnerId)}`, Runner.GetRunnerResponseSchema)
  }

  deleteRunner(runnerId: string): Promise<Runner.DeleteRunnerResponse> {
    return this.#request('DELETE', `/apis/auv/runtime/v1/runners/${encodeURIComponent(runnerId)}`, Runner.DeleteRunnerResponseSchema)
  }

  async #post<RequestSchema extends DescMessage, ResponseSchema extends DescMessage>(
    path: string,
    requestSchema: RequestSchema,
    request: MessageShape<RequestSchema>,
    responseSchema: ResponseSchema,
  ): Promise<MessageShape<ResponseSchema>> {
    return this.#request('POST', path, responseSchema, toBinary(requestSchema, request))
  }

  async #request<ResponseSchema extends DescMessage>(
    method: 'GET' | 'POST' | 'DELETE',
    path: string,
    responseSchema: ResponseSchema,
    body?: Uint8Array,
  ): Promise<MessageShape<ResponseSchema>> {
    const response = await this.#fetch(new URL(path, this.#baseUrl), {
      method,
      headers: body === undefined ? undefined : { 'content-type': 'application/protobuf' },
      body: body === undefined ? undefined : Uint8Array.from(body).buffer,
    })
    if (!response.ok) {
      throw await AuvRestError.fromResponse(response)
    }
    const contentType = response.headers.get('content-type')?.split(';', 1)[0]
    if (contentType !== 'application/protobuf') {
      throw new Error(`AUV REST response used unexpected content type ${contentType ?? '<missing>'}`)
    }
    return fromBinary(responseSchema, new Uint8Array(await response.arrayBuffer()))
  }
}

export class AuvRestError extends Error {
  constructor(
    readonly status: number,
    readonly problem: unknown,
  ) {
    super(`AUV REST request failed with HTTP ${status}`)
  }

  static async fromResponse(response: Response): Promise<AuvRestError> {
    const contentType = response.headers.get('content-type')?.split(';', 1)[0]
    const problem = contentType === 'application/problem+json' ? await response.json() : await response.text()
    return new AuvRestError(response.status, problem)
  }
}

export function createAuvRestClient(endpoint: string, options?: AuvRestClientOptions): AuvRestClient {
  return new AuvRestClient(endpoint, options)
}

function validatedRestUrl(endpoint: string): URL {
  const url = new URL(endpoint)
  const localHttp = url.protocol === 'http:' && isLoopback(url.hostname)
  if ((!localHttp && url.protocol !== 'https:') || url.pathname !== '/' || url.search !== '' || url.hash !== '') {
    throw new Error(`AUV REST requires pathless loopback HTTP or paired HTTPS, received ${endpoint}`)
  }
  return url
}

function isLoopback(hostname: string): boolean {
  return hostname === 'localhost' || hostname === '127.0.0.1' || hostname === '[::1]'
}
