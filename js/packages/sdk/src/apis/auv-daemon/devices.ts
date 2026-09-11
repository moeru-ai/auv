import type { V1GetDeviceRequest } from '@auv-js/api-client'

import type { Device as ProtoDevice } from '../../gen/auv/api/daemon/v1/device_pb'
import type { AuvConnection, RpcDefinition } from '../../transport/connection'
import type { OperationOptions } from '../../transport/types'

import {
  deviceServiceGetDevice,
  deviceServiceListDevices,
} from '@auv-js/api-client'

import {
  DeviceService,
  GetDeviceRequestSchema,
  GetDeviceResponseSchema,
  ListDevicesRequestSchema,
  ListDevicesResponseSchema,
  DevicePlatform as ProtoDevicePlatform,
} from '../../gen/auv/api/daemon/v1/device_pb'
import { AuvProtocolError } from '../../transport/errors'
import { unknownEnum } from './wire'

/** One Device visible to the connected AUV caller. */
export interface Device {
  id: string
  labels: Readonly<Record<string, string>>
  local: boolean
  name: string
  platform: DevicePlatform
}

export type DevicePlatform = 'linux' | 'macos' | 'unspecified' | 'windows'

export interface GetDeviceOptions extends OperationOptions {
  deviceId: string
}

const listDevicesRpc = {
  input: ListDevicesRequestSchema,
  method: `/${DeviceService.typeName}/${DeviceService.method.listDevices.name}`,
  output: ListDevicesResponseSchema,
  rest: ({ client, headers, signal }) => deviceServiceListDevices({ client, headers, signal }),
} satisfies RpcDefinition<typeof ListDevicesRequestSchema, typeof ListDevicesResponseSchema>

const getDeviceRpc = {
  input: GetDeviceRequestSchema,
  method: `/${DeviceService.typeName}/${DeviceService.method.getDevice.name}`,
  output: GetDeviceResponseSchema,
  rest: ({ body, ...options }) => deviceServiceGetDevice({ body: body as V1GetDeviceRequest, ...options }),
} satisfies RpcDefinition<typeof GetDeviceRequestSchema, typeof GetDeviceResponseSchema>

/** Gets one Device by canonical identity. */
export async function getDevice(connection: AuvConnection, options: GetDeviceOptions): Promise<Device> {
  const response = await connection.unary(getDeviceRpc, { device: { deviceId: options.deviceId } }, options)
  if (response.device === undefined) {
    throw new AuvProtocolError('AUV response omitted GetDeviceResponse.device')
  }
  return device(response.device)
}

/** Lists Devices visible to the connected caller. */
export async function listDevices(connection: AuvConnection, options: OperationOptions = {}): Promise<readonly Device[]> {
  const response = await connection.unary(listDevicesRpc, {}, options)
  return response.devices.map(device)
}

function device(value: ProtoDevice): Device {
  const id = value.ref?.deviceId
  if (id === undefined || id.length === 0) {
    throw new AuvProtocolError('AUV response omitted Device.ref.device_id')
  }
  return {
    id,
    labels: value.labels,
    local: value.local,
    name: value.name,
    platform: devicePlatform(value.platform),
  }
}

function devicePlatform(value: ProtoDevicePlatform): DevicePlatform {
  switch (value) {
    case ProtoDevicePlatform.LINUX:
      return 'linux'
    case ProtoDevicePlatform.MACOS:
      return 'macos'
    case ProtoDevicePlatform.UNSPECIFIED:
      return 'unspecified'
    case ProtoDevicePlatform.WINDOWS:
      return 'windows'
    default:
      return unknownEnum('Device.platform', value)
  }
}
