import * as sdk from '@auv-js/sdk'

/** The SDK surface admitted into the agent VM. Help uses this same selection. */
export const codeModeSdk = {
  above: sdk.above,
  at: sdk.at,
  below: sdk.below,
  CaptureResolution: sdk.CaptureResolution,
  center: sdk.center,
  contains: sdk.contains,
  ImageEncoding: sdk.ImageEncoding,
  InputPolicy: sdk.InputPolicy,
  inset: sdk.inset,
  intersect: sdk.intersect,
  leftOf: sdk.leftOf,
  MouseButton: sdk.MouseButton,
  offset: sdk.offset,
  Position: sdk.Position,
  region: sdk.region,
  rightOf: sdk.rightOf,
  ScrollUntilStopReason: sdk.ScrollUntilStopReason,
}
