import { createClient, type Client } from '@connectrpc/connect'
import { createGrpcTransport } from '@connectrpc/connect-node'

import { DeviceService } from './gen/auv/api/core/v1/device_pb.js'
import { RunnerClassService, RunnerService } from './gen/auv/api/core/v1/runner_pb.js'
import { RunService } from './gen/auv/api/core/v1/run_pb.js'
import { DisplayService } from './gen/auv/api/driver/v1/display_pb.js'
import { CaptureService } from './gen/auv/api/driver/v1/capture_pb.js'
import { InputService } from './gen/auv/api/driver/v1/input_pb.js'
import { TextRecognitionService } from './gen/auv/api/driver/v1/text_recognition_pb.js'
import { WindowService } from './gen/auv/api/driver/v1/window_pb.js'
import { PermissionService } from './gen/auv/api/driver/macos/v1/permission_pb.js'
import { ApplicationService } from './gen/auv/api/driver/macos/v1/application_pb.js'
import { ObjectDetectionService } from './gen/auv/api/inference/v1/object_detection_pb.js'

export * from './gen/auv/api/core/v1/device_pb.js'
export * from './gen/auv/api/core/v1/discovery_pb.js'
export * from './gen/auv/api/core/v1/resource_pb.js'
export * from './gen/auv/api/core/v1/run_pb.js'
export * from './gen/auv/api/core/v1/runner_pb.js'
export * from './gen/auv/api/driver/v1/display_pb.js'
export * from './gen/auv/api/driver/v1/capture_pb.js'
export * from './gen/auv/api/driver/v1/geometry_pb.js'
export * from './gen/auv/api/driver/v1/input_pb.js'
export * from './gen/auv/api/driver/v1/text_recognition_pb.js'
export * from './gen/auv/api/driver/v1/window_pb.js'
export * from './gen/auv/api/driver/macos/v1/permission_pb.js'
export * from './gen/auv/api/driver/macos/v1/application_pb.js'
export * from './gen/auv/api/inference/v1/object_detection_pb.js'
export * from './gen/auv/api/image/v1/image_pb.js'
export * from './gen/auv/api/image/v1/region_pb.js'
export * from './rest.js'

export type AuvDeviceClient = Client<typeof DeviceService>
export type AuvRunnerClient = Client<typeof RunnerService>
export type AuvRunnerClassClient = Client<typeof RunnerClassService>
export type AuvRunClient = Client<typeof RunService>
export type AuvDisplayClient = Client<typeof DisplayService>
export type AuvCaptureClient = Client<typeof CaptureService>
export type AuvInputClient = Client<typeof InputService>
export type AuvTextRecognitionClient = Client<typeof TextRecognitionService>
export type AuvWindowClient = Client<typeof WindowService>
export type AuvMacosPermissionClient = Client<typeof PermissionService>
export type AuvMacosApplicationClient = Client<typeof ApplicationService>
export type AuvObjectDetectionClient = Client<typeof ObjectDetectionService>

export function createAuvDeviceClient(endpoint: string): AuvDeviceClient {
  const url = validatedLoopbackUrl(endpoint)
  return createClient(DeviceService, createGrpcTransport({ baseUrl: url.origin }))
}

export function createAuvRunnerClient(endpoint: string): AuvRunnerClient {
  const url = validatedLoopbackUrl(endpoint)
  return createClient(RunnerService, createGrpcTransport({ baseUrl: url.origin }))
}

export function createAuvRunnerClassClient(endpoint: string): AuvRunnerClassClient {
  const url = validatedLoopbackUrl(endpoint)
  return createClient(RunnerClassService, createGrpcTransport({ baseUrl: url.origin }))
}

export function createAuvRunClient(endpoint: string): AuvRunClient {
  const url = validatedLoopbackUrl(endpoint)
  return createClient(RunService, createGrpcTransport({ baseUrl: url.origin }))
}

export function createAuvDisplayClient(endpoint: string): AuvDisplayClient {
  const url = validatedLoopbackUrl(endpoint)
  return createClient(DisplayService, createGrpcTransport({ baseUrl: url.origin }))
}

export function createAuvCaptureClient(endpoint: string): AuvCaptureClient {
  const url = validatedLoopbackUrl(endpoint)
  return createClient(CaptureService, createGrpcTransport({ baseUrl: url.origin }))
}

export function createAuvInputClient(endpoint: string): AuvInputClient {
  const url = validatedLoopbackUrl(endpoint)
  return createClient(InputService, createGrpcTransport({ baseUrl: url.origin }))
}

export function createAuvTextRecognitionClient(endpoint: string): AuvTextRecognitionClient {
  const url = validatedLoopbackUrl(endpoint)
  return createClient(TextRecognitionService, createGrpcTransport({ baseUrl: url.origin }))
}

export function createAuvWindowClient(endpoint: string): AuvWindowClient {
  const url = validatedLoopbackUrl(endpoint)
  return createClient(WindowService, createGrpcTransport({ baseUrl: url.origin }))
}

export function createAuvMacosPermissionClient(endpoint: string): AuvMacosPermissionClient {
  const url = validatedLoopbackUrl(endpoint)
  return createClient(PermissionService, createGrpcTransport({ baseUrl: url.origin }))
}

export function createAuvMacosApplicationClient(endpoint: string): AuvMacosApplicationClient {
  const url = validatedLoopbackUrl(endpoint)
  return createClient(ApplicationService, createGrpcTransport({ baseUrl: url.origin }))
}

export function createAuvObjectDetectionClient(endpoint: string): AuvObjectDetectionClient {
  const url = validatedLoopbackUrl(endpoint)
  return createClient(ObjectDetectionService, createGrpcTransport({ baseUrl: url.origin }))
}

function validatedLoopbackUrl(endpoint: string): URL {
  const url = new URL(endpoint)
  if (url.protocol !== 'http:' || !isLoopback(url.hostname) || url.pathname !== '/') {
    throw new Error(`AUV currently accepts only pathless loopback HTTP endpoints, received ${endpoint}`)
  }
  return url
}

function isLoopback(hostname: string): boolean {
  return hostname === 'localhost' || hostname === '127.0.0.1' || hostname === '[::1]'
}

// TODO(js-unix-transport): add Unix-socket dialing only with a tested Node
// connector; browser clients require an explicit gRPC-Web/Connect gateway.
