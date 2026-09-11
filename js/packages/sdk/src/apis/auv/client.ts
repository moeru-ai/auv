import type { DescMessage, MessageShape } from '@bufbuild/protobuf'

import type { AuvConnection } from '../../transport/connection'
import type { OperationOptions } from '../../transport/types'
import type { Device, GetDeviceOptions } from '../auv-daemon/devices'
import type { HealthStatus } from '../auv-daemon/health'
import type { CreatePairingTokenOptions, PairDeviceOptions, PairedDeviceOptions, PairingEnrollment, PairingToken, SetPairedDeviceEnabledOptions } from '../auv-daemon/pairing'
import type { CreateRunnerOptions, DeleteRunnerOptions, GetRunnerClassOptions, GetRunnerOptions, ListRunnerClassesOptions, Runner, RunnerClass } from '../auv-daemon/runners'
import type { CreateRunOptions, GetRunOptions, Run, StopRunOptions } from '../auv-daemon/runs'
import type { DiscoveredRunner, DiscoverRunnerOptions } from './discover'
import type { RunnerClient, RunnerRouteOptions } from './driver'
import type { InvokeDuplexOptions, InvokeServerStreamOptions, InvokeUnaryOptions } from './invoke'

import { getDevice, listDevices } from '../auv-daemon/devices'
import { checkHealth } from '../auv-daemon/health'
import { createPairingToken, pairDevice, revokeDeviceCredential, setPairedDeviceEnabled, unpairDevice } from '../auv-daemon/pairing'
import { createRunner, deleteRunner, getRunner, getRunnerClass, listRunnerClasses, listRunners } from '../auv-daemon/runners'
import { createRun, getRun, listRuns, stopRun } from '../auv-daemon/runs'
import { discoverRunner } from './discover'
import { createRunnerClient } from './driver'
import { invokeDuplex, invokeServerStream, invokeUnary } from './invoke'

export interface AuvClient {
  readonly connection: AuvConnection
  readonly devices: {
    get: (options: GetDeviceOptions) => Promise<Device>
    list: (options?: OperationOptions) => Promise<readonly Device[]>
  }
  readonly health: {
    check: (options?: OperationOptions) => Promise<HealthStatus>
  }
  readonly invoke: {
    duplex: <I extends DescMessage, O extends DescMessage>(options: InvokeDuplexOptions<I, O>) => ReturnType<typeof invokeDuplex<I, O>>
    serverStream: <I extends DescMessage, O extends DescMessage>(options: InvokeServerStreamOptions<I, O>) => Promise<AsyncIterable<MessageShape<O>>>
    unary: <I extends DescMessage, O extends DescMessage>(options: InvokeUnaryOptions<I, O>) => Promise<MessageShape<O>>
  }
  readonly pairing: {
    createToken: (options?: CreatePairingTokenOptions) => Promise<PairingToken>
    pair: (options: PairDeviceOptions) => Promise<PairingEnrollment>
    revokeCredential: (options: PairedDeviceOptions) => Promise<boolean>
    setEnabled: (options: SetPairedDeviceEnabledOptions) => Promise<boolean>
    unpair: (options: PairedDeviceOptions) => Promise<boolean>
  }
  /** Binds first-party Driver capabilities to one Runner route. */
  readonly runner: (options: RunnerRouteOptions) => RunnerClient
  readonly runners: {
    create: (options: CreateRunnerOptions) => Promise<Runner>
    delete: (options: DeleteRunnerOptions) => Promise<Runner>
    discover: (options: DiscoverRunnerOptions) => Promise<DiscoveredRunner>
    get: (options: GetRunnerOptions) => Promise<Runner>
    getClass: (options: GetRunnerClassOptions) => Promise<RunnerClass>
    list: (options?: OperationOptions) => Promise<readonly Runner[]>
    listClasses: (options?: ListRunnerClassesOptions) => Promise<readonly RunnerClass[]>
  }
  readonly runs: {
    create: (options?: CreateRunOptions) => Promise<Run>
    get: (options: GetRunOptions) => Promise<Run>
    list: (options?: OperationOptions) => Promise<readonly Run[]>
    stop: (options: StopRunOptions) => Promise<Run>
  }
}

export interface CreateClientOptions {
  signal?: AbortSignal
}

/** Binds an AUV connection and optional default operation lifetime. */
export function createAuv(connection: AuvConnection, options: CreateClientOptions = {}): AuvClient {
  const operation = <T extends OperationOptions>(value?: T): OperationOptions & T => ({
    ...value as T,
    signal: combineSignals(options.signal, value?.signal),
  })
  return {
    connection,
    devices: {
      get: value => getDevice(connection, operation(value)),
      list: value => listDevices(connection, operation(value)),
    },
    health: {
      check: value => checkHealth(connection, operation(value)),
    },
    invoke: {
      duplex: value => invokeDuplex(connection, operation(value)),
      serverStream: value => invokeServerStream(connection, operation(value)),
      unary: value => invokeUnary(connection, operation(value)),
    },
    pairing: {
      createToken: value => createPairingToken(connection, operation(value)),
      pair: value => pairDevice(connection, operation(value)),
      revokeCredential: value => revokeDeviceCredential(connection, operation(value)),
      setEnabled: value => setPairedDeviceEnabled(connection, operation(value)),
      unpair: value => unpairDevice(connection, operation(value)),
    },
    runner: value => createRunnerClient(connection, operation(value)),
    runners: {
      create: value => createRunner(connection, operation(value)),
      delete: value => deleteRunner(connection, operation(value)),
      discover: value => discoverRunner(connection, operation(value)),
      get: value => getRunner(connection, operation(value)),
      getClass: value => getRunnerClass(connection, operation(value)),
      list: value => listRunners(connection, operation(value)),
      listClasses: value => listRunnerClasses(connection, operation(value)),
    },
    runs: {
      create: value => createRun(connection, operation(value)),
      get: value => getRun(connection, operation(value)),
      list: value => listRuns(connection, operation(value)),
      stop: value => stopRun(connection, operation(value)),
    },
  }
}

function combineSignals(first?: AbortSignal, second?: AbortSignal): AbortSignal | undefined {
  if (first === undefined)
    return second
  if (second === undefined)
    return first
  return AbortSignal.any([first, second])
}
