import { createSandboxRunner, installSandboxTimers } from '@auv-js/sandbox-bridge'

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

installSandboxTimers()
export const auv = createSandboxRunner()
