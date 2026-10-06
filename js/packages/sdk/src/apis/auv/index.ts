// Driver enums callers pass or read; without them clients mirror raw numbers.
export { PermissionStatus } from '../../gen/auv/api/driver/macos/v1/permission_pb'
export {
  DisturbanceLevel,
  InputDeliveryPath,
  InputPolicy,
  MouseButton,
  ScrollDeliveryCandidate,
  ScrollStreamStopReason,
  ScrollUntilStopReason,
  StandardMotionTimingFunction,
  TextSubmit,
  WindowClickStrategy,
} from '../../gen/auv/api/driver/v1/input_pb'

export { BuiltInCursor, Easing } from '../../gen/auv/api/driver/v1/overlay_pb'
export { createAuv } from './client'

export type { AuvClient, CreateClientOptions } from './client'
export { discoverRunner } from './discover'
export type {
  DiscoveredMethodEffect,
  DiscoveredRpcMethod,
  DiscoveredRunner,
  DiscoverRunnerOptions,
  InvokeDiscoveredOptions,
} from './discover'
export { createRunnerClient } from './driver'
export type {
  FindDisplayTextOptions,
  FindWindowTextOptions,
  PressKeyOptions,
  RecognizeTextOptions,
  RunnerClient,
  RunnerRouteOptions,
  ScrollStreamBegin,
  ScrollStreamController,
  ScrollUntilCallOptions,
  ScrollUntilOptions,
  ScrollWithStep,
  WindowClient,
  WindowTarget,
} from './driver'
export { invokeDuplex, invokeServerStream, invokeUnary } from './invoke'
export type { InvokeDuplexOptions, InvokeServerStreamOptions, InvokeUnaryOptions } from './invoke'
export { protobufJsonSchema } from './json'
export type { ProtobufJsonSchema } from './json'
