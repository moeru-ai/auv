import type { MessageInitShape } from '@bufbuild/protobuf'

import type { RunSchema } from '../gen/auv/api/daemon/v1/run_pb'
import type { MockRouter } from './transport'

import { timestampNow } from '@bufbuild/protobuf/wkt'

import { DevicePlatform, DeviceService } from '../gen/auv/api/daemon/v1/device_pb'
import { RunOutcome, RunPhase, RunService } from '../gen/auv/api/daemon/v1/run_pb'
import { AuvRpcError } from '../transport/errors'

/** The one Device a mock daemon reports. */
export interface MockDevice {
  id: string
  labels?: Record<string, string>
  name: string
  platform?: DevicePlatform
}

// gRPC status code NOT_FOUND.
const NOT_FOUND = 5

/**
 * Answers the daemon calls a client makes around Runner calls: listing the
 * one local Device and the Run lifecycle (create, get, list, stop). Runs are
 * correlation only; nothing is recorded.
 */
export function serveMockDaemon({ service }: MockRouter, device: MockDevice): void {
  const proto = {
    labels: device.labels ?? {},
    local: true,
    name: device.name,
    platform: device.platform ?? DevicePlatform.UNSPECIFIED,
    ref: { deviceId: device.id },
  }
  service(DeviceService, {
    getDevice: (request) => {
      if (request.device?.deviceId && request.device.deviceId !== device.id)
        throw new AuvRpcError(NOT_FOUND, `Device ${request.device.deviceId} was not found`)
      return { device: proto }
    },
    listDevices: () => ({ devices: [proto] }),
  })

  const runs = new Map<string, MessageInitShape<typeof RunSchema>>()
  let next = 0
  const known = (runId: string | undefined) => {
    const run = runId ? runs.get(runId) : undefined
    if (!run)
      throw new AuvRpcError(NOT_FOUND, `Run ${runId ?? ''} was not found`)
    return run
  }
  service(RunService, {
    createRun: (request) => {
      // Run IDs are 32 hex characters, like the daemon's.
      const runId = (++next).toString(16).padStart(32, '0')
      const run = { createdAt: timestampNow(), devices: [{ deviceId: device.id }], labels: request.labels, phase: RunPhase.RUNNING, ref: { runId } }
      runs.set(runId, run)
      return { run }
    },
    getRun: request => ({ run: known(request.run?.runId) }),
    listRuns: () => ({ runs: [...runs.values()] }),
    stopRun: (request) => {
      const run = known(request.run?.runId)
      run.phase = request.outcome === RunOutcome.FAILED ? RunPhase.FAILED : request.outcome === RunOutcome.CANCELED ? RunPhase.CANCELED : RunPhase.SUCCEEDED
      run.completedAt = timestampNow()
      return { run }
    },
  })
}
