import CoreGraphics
import CoreImage
import CoreMedia
import CoreVideo
import Foundation
import ScreenCaptureKit

private func emptyWindowCaptureResponse(
  message: String,
  recovery: String
) -> NativeWindowCaptureResponse {
  NativeWindowCaptureResponse(
    image_width: 0,
    image_height: 0,
    rgba_bytes: RustVec<UInt8>(),
    error_message: message.intoRustString(),
    recovery_hint: recovery.intoRustString()
  )
}

func capture_window_image(request: NativeWindowCaptureRequest) -> NativeWindowCaptureResponse {
  var capturedImage: CGImage?
  var captureError: Error?
  let status = nativeCaptureWindowForAuv(windowID: UInt32(max(request.window_id, 0)), logical: request.logical) { image, error in
    capturedImage = image
    captureError = error
  }
  if status == .timedOut {
    return emptyWindowCaptureResponse(
      message: "ScreenCaptureKit window capture timed out after 10s",
      recovery: "verify Screen Recording permission and retry"
    )
  }
  if let captureError {
    return emptyWindowCaptureResponse(
      message: "ScreenCaptureKit window capture failed: \(captureError)",
      recovery: "verify the target window is capturable and retry"
    )
  }
  guard let image = capturedImage else {
    return emptyWindowCaptureResponse(
      message: "ScreenCaptureKit returned no window image",
      recovery: "retry capture or use a fallback capture method"
    )
  }
  guard let rgba = nativeRgbaBytes(from: image) else {
    return emptyWindowCaptureResponse(
      message: "failed to extract RGBA bytes from captured window",
      recovery: "retry capture or use a fallback capture method"
    )
  }
  return NativeWindowCaptureResponse(
    image_width: Int64(image.width),
    image_height: Int64(image.height),
    rgba_bytes: nativeByteVec(rgba),
    error_message: nil,
    recovery_hint: nil
  )
}

private func nativeCaptureWindowForAuv(
  windowID: UInt32,
  logical: Bool,
  completion: @escaping (CGImage?, Error?) -> Void
) -> DispatchTimeoutResult {
  let semaphore = DispatchSemaphore(value: 0)

  guard #available(macOS 14.0, *) else {
    completion(nil, NSError(
      domain: "AuvMacosNative.Capture",
      code: 3,
      userInfo: [NSLocalizedDescriptionKey: "ScreenCaptureKit screenshot capture requires macOS 14.0 or newer"]
    ))
    semaphore.signal()
    return semaphore.wait(timeout: .now() + .seconds(10))
  }

  SCShareableContent.getWithCompletionHandler { content, error in
    if let error {
      completion(nil, error)
      semaphore.signal()
      return
    }
    guard let window = content?.windows.first(where: { $0.windowID == windowID }) else {
      completion(nil, NSError(
        domain: "AuvMacosNative.Capture",
        code: 1,
        userInfo: [NSLocalizedDescriptionKey: "window \(windowID) not found"]
      ))
      semaphore.signal()
      return
    }

    let filter = SCContentFilter(desktopIndependentWindow: window)
    let config = SCStreamConfiguration()
    // NOTICE(window-capture-pixel-scale): SCStreamConfiguration.width/height
    // are output pixels, while SCWindow.frame is in points. Passing points
    // captured Retina windows at 1x while display captures were 2x (measured
    // 2026-10-07). `pointPixelScale` (macOS 14+) is the backing scale of the
    // filtered content; see
    // https://developer.apple.com/documentation/screencapturekit/scshareablecontentinfo.
    let scale = logical ? 1.0 : CGFloat(SCShareableContent.info(for: filter).pointPixelScale)
    config.width = max(1, Int((window.frame.width * scale).rounded()))
    config.height = max(1, Int((window.frame.height * scale).rounded()))
    config.pixelFormat = kCVPixelFormatType_32BGRA
    config.colorSpaceName = CGColorSpace.sRGB
    config.showsCursor = false

    SCScreenshotManager.captureSampleBuffer(
      contentFilter: filter,
      configuration: config
    ) { sampleBuffer, captureError in
      if let captureError {
        completion(nil, captureError)
        semaphore.signal()
        return
      }
      guard
        let sampleBuffer,
        let image = nativeImageFromSampleBuffer(sampleBuffer)
      else {
        completion(nil, NSError(
          domain: "AuvMacosNative.Capture",
          code: 2,
          userInfo: [NSLocalizedDescriptionKey: "window capture returned no image sample"]
        ))
        semaphore.signal()
        return
      }
      completion(image, nil)
      semaphore.signal()
    }
  }

  return semaphore.wait(timeout: .now() + .seconds(10))
}

func nativeImageFromSampleBuffer(_ sampleBuffer: CMSampleBuffer) -> CGImage? {
  guard let pixelBuffer = CMSampleBufferGetImageBuffer(sampleBuffer) else {
    return nil
  }
  let ciImage = CIImage(cvPixelBuffer: pixelBuffer)
  return CIContext(options: nil).createCGImage(ciImage, from: ciImage.extent)
}

func nativeRgbaBytes(from image: CGImage) -> [UInt8]? {
  let width = image.width
  let height = image.height
  var bytes = [UInt8](repeating: 0, count: width * height * 4)
  guard
    let context = CGContext(
      data: &bytes,
      width: width,
      height: height,
      bitsPerComponent: 8,
      bytesPerRow: width * 4,
      space: CGColorSpaceCreateDeviceRGB(),
      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    )
  else {
    return nil
  }
  context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
  return bytes
}

func nativeImageFromRgbaBytes(width: Int, height: Int, bytes: [UInt8]) -> CGImage? {
  guard width > 0, height > 0, bytes.count == width * height * 4 else {
    return nil
  }
  let data = Data(bytes)
  guard let provider = CGDataProvider(data: data as CFData) else {
    return nil
  }
  return CGImage(
    width: width,
    height: height,
    bitsPerComponent: 8,
    bitsPerPixel: 32,
    bytesPerRow: width * 4,
    space: CGColorSpaceCreateDeviceRGB(),
    bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
    provider: provider,
    decode: nil,
    shouldInterpolate: false,
    intent: .defaultIntent
  )
}
