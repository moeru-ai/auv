import { AuvConfigurationError } from '../../transport/errors'

export function selectedDevice(local: boolean, deviceId?: string): string | undefined {
  if (local && deviceId !== undefined) {
    throw new AuvConfigurationError('local connection cannot select an explicit Device')
  }

  return deviceId
}

export function selectedDevices(local: boolean, deviceIds?: readonly string[]): readonly string[] | undefined {
  if (local && deviceIds !== undefined && deviceIds.length !== 0) {
    throw new AuvConfigurationError('local connection cannot select explicit Devices')
  }

  return deviceIds
}
