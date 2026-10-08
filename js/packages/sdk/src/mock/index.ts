// The Runner's generated service descriptors, for `service(WindowService, …)`
// in a mock Runner and for typed access to their methods.
export { AccessibilityService } from '../gen/auv/api/driver/macos/v1/accessibility_pb'
export { ApplicationService } from '../gen/auv/api/driver/macos/v1/application_pb'
export { MediaControlService } from '../gen/auv/api/driver/macos/v1/media_control_pb'
export { PermissionService } from '../gen/auv/api/driver/macos/v1/permission_pb'

export { CaptureService } from '../gen/auv/api/driver/v1/capture_pb'
export { DisplayService } from '../gen/auv/api/driver/v1/display_pb'
export { InputService } from '../gen/auv/api/driver/v1/input_pb'
export { OverlayService } from '../gen/auv/api/driver/v1/overlay_pb'
export { RecentFramesService } from '../gen/auv/api/driver/v1/recent_frames_pb'
export { TextRecognitionService } from '../gen/auv/api/driver/v1/text_recognition_pb'
export { WindowService } from '../gen/auv/api/driver/v1/window_pb'
export { serveMockDaemon } from './daemon'
export type { MockDevice } from './daemon'
export { createMockTransport } from './transport'
export type { MockCall, MockMethod, MockRouter, MockService } from './transport'
