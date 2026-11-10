import CoreGraphics
import Foundation

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
