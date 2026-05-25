import CoreGraphics
import Foundation
import ImageIO
import Vision

private func emptyOcrTextResponse(request: NativeOcrTextRequest, message: String, recovery: String) -> NativeOcrTextResponse {
  NativeOcrTextResponse(
    recognized_at: nativeNowIso8601(),
    image_path: request.image_path,
    image_width: 0,
    image_height: 0,
    query: request.query,
    exact: request.exact,
    case_sensitive: request.case_sensitive,
    normalized_query: "".intoRustString(),
    crop_enabled: request.crop_enabled,
    crop_x: request.crop_x,
    crop_y: request.crop_y,
    crop_width: request.crop_width,
    crop_height: request.crop_height,
    ocr_scale_factor: 1.0,
    match_indices: RustVec<Int64>(),
    texts: RustVec<RustString>(),
    confidences: RustVec<Double>(),
    x_values: RustVec<Int64>(),
    y_values: RustVec<Int64>(),
    width_values: RustVec<Int64>(),
    height_values: RustVec<Int64>(),
    error_message: message.intoRustString(),
    recovery_hint: recovery.intoRustString()
  )
}

private func emptyVisualRowsResponse(
  request: NativeVisualRowsRequest,
  message: String,
  recovery: String
) -> NativeVisualRowsResponse {
  NativeVisualRowsResponse(
    detected_at: nativeNowIso8601(),
    image_path: request.image_path,
    image_width: 0,
    image_height: 0,
    crop_enabled: request.crop_enabled,
    crop_x: request.crop_x,
    crop_y: request.crop_y,
    crop_width: request.crop_width,
    crop_height: request.crop_height,
    analysis_strip_x: 0,
    analysis_strip_y: 0,
    analysis_strip_width: 0,
    analysis_strip_height: 0,
    row_indices: RustVec<Int64>(),
    x_values: RustVec<Int64>(),
    y_values: RustVec<Int64>(),
    width_values: RustVec<Int64>(),
    height_values: RustVec<Int64>(),
    peak_densities: RustVec<Double>(),
    error_message: message.intoRustString(),
    recovery_hint: recovery.intoRustString()
  )
}

private func loadImage(path: String) -> CGImage? {
  let imageURL = URL(fileURLWithPath: path)
  guard
    let imageSource = CGImageSourceCreateWithURL(imageURL as CFURL, nil),
    let image = CGImageSourceCreateImageAtIndex(imageSource, 0, nil)
  else {
    return nil
  }
  return image
}

private func upscale(_ image: CGImage, factor: CGFloat) -> CGImage? {
  let width = Int((CGFloat(image.width) * factor).rounded())
  let height = Int((CGFloat(image.height) * factor).rounded())
  guard
    let colorSpace = image.colorSpace ?? CGColorSpace(name: CGColorSpace.sRGB),
    let context = CGContext(
      data: nil,
      width: width,
      height: height,
      bitsPerComponent: image.bitsPerComponent,
      bytesPerRow: 0,
      space: colorSpace,
      bitmapInfo: image.bitmapInfo.rawValue
    )
  else {
    return nil
  }

  context.interpolationQuality = .high
  context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
  return context.makeImage()
}

private func foldConfusableScalar(_ scalar: UnicodeScalar) -> UnicodeScalar {
  switch scalar {
  case "|", "!", "l", "I":
    return "i"
  default:
    return scalar
  }
}

private func normalizeForAnchorMatch(_ raw: String, caseSensitive: Bool) -> String {
  let sanitized = nativeSanitize(raw)
  let folded = String(String.UnicodeScalarView(sanitized.unicodeScalars.map(foldConfusableScalar)))
  let lowercased = caseSensitive ? folded : folded.lowercased()
  return String(
    lowercased.unicodeScalars.filter { scalar in
      CharacterSet.alphanumerics.contains(scalar)
    }
  )
}

func find_ocr_text(request: NativeOcrTextRequest) -> NativeOcrTextResponse {
  let imagePath = request.image_path.toString()
  let rawQuery = request.query.toString()
  guard let image = loadImage(path: imagePath) else {
    return emptyOcrTextResponse(
      request: request,
      message: "could not load image for OCR at \(imagePath)",
      recovery: "verify the image artifact path exists and is readable"
    )
  }

  var workingImage = image
  var cropOffsetX: Int64 = 0
  var cropOffsetY: Int64 = 0
  var ocrScaleFactor: CGFloat = 1.0

  if request.crop_enabled {
    let cropRect = CGRect(
      x: Int(request.crop_x),
      y: Int(request.crop_y),
      width: Int(request.crop_width),
      height: Int(request.crop_height)
    ).integral
    guard cropRect.width > 0, cropRect.height > 0 else {
      return emptyOcrTextResponse(
        request: request,
        message: "invalid OCR crop rect \(cropRect)",
        recovery: "pass a positive crop region inside the image bounds"
      )
    }
    guard let croppedImage = image.cropping(to: cropRect) else {
      return emptyOcrTextResponse(
        request: request,
        message: "could not crop OCR image to \(cropRect)",
        recovery: "pass a crop region inside the image bounds"
      )
    }
    cropOffsetX = Int64(cropRect.origin.x.rounded())
    cropOffsetY = Int64(cropRect.origin.y.rounded())
    if let upscaledImage = upscale(croppedImage, factor: 2.0) {
      workingImage = upscaledImage
      ocrScaleFactor = 2.0
    } else {
      workingImage = croppedImage
    }
  }

  let normalizedQuery = request.case_sensitive ? rawQuery : rawQuery.lowercased()
  let normalizedAnchorQuery = normalizeForAnchorMatch(rawQuery, caseSensitive: request.case_sensitive)

  func matches(_ text: String) -> Bool {
    if rawQuery.isEmpty {
      return true
    }
    let normalizedText = request.case_sensitive ? text : text.lowercased()
    let normalizedAnchorText = normalizeForAnchorMatch(text, caseSensitive: request.case_sensitive)
    if request.exact {
      return normalizedText == normalizedQuery || normalizedAnchorText == normalizedAnchorQuery
    }
    return normalizedText.contains(normalizedQuery)
      || normalizedAnchorText.contains(normalizedAnchorQuery)
  }

  let visionRequest = VNRecognizeTextRequest()
  visionRequest.recognitionLevel = .accurate
  visionRequest.usesLanguageCorrection = true
  visionRequest.recognitionLanguages = ["zh-Hans", "zh-Hant", "en-US"]
  if !rawQuery.isEmpty {
    visionRequest.customWords = [rawQuery]
  }
  if #available(macOS 26.0, *) {
    visionRequest.automaticallyDetectsLanguage = true
  }

  let handler = VNImageRequestHandler(cgImage: workingImage, options: [:])
  do {
    try handler.perform([visionRequest])
  } catch {
    return emptyOcrTextResponse(
      request: request,
      message: "vision OCR failed: \(error)",
      recovery: "verify Screen Recording/capture output and retry"
    )
  }

  let observations = (visionRequest.results as? [VNRecognizedTextObservation]) ?? []
  let maxObservations = max(Int(request.max_observations), 1)

  var matchIndices: [Int64] = []
  var texts: [String] = []
  var confidences: [Double] = []
  var xValues: [Int64] = []
  var yValues: [Int64] = []
  var widthValues: [Int64] = []
  var heightValues: [Int64] = []

  for observation in observations.prefix(maxObservations) {
    let candidates = observation.topCandidates(5)
    guard
      let candidate = candidates.first(where: { candidate in
        let text = nativeSanitize(candidate.string)
        return !text.isEmpty && matches(text)
      })
    else { continue }
    let text = nativeSanitize(candidate.string)

    let boundingBox = observation.boundingBox
    let workingX = Int64((boundingBox.minX * CGFloat(workingImage.width)).rounded())
    let workingY = Int64(((1.0 - boundingBox.maxY) * CGFloat(workingImage.height)).rounded())
    let workingWidth = Int64((boundingBox.width * CGFloat(workingImage.width)).rounded())
    let workingHeight = Int64((boundingBox.height * CGFloat(workingImage.height)).rounded())

    let x = cropOffsetX + Int64((CGFloat(workingX) / ocrScaleFactor).rounded())
    let y = cropOffsetY + Int64((CGFloat(workingY) / ocrScaleFactor).rounded())
    let width = Int64((CGFloat(workingWidth) / ocrScaleFactor).rounded())
    let height = Int64((CGFloat(workingHeight) / ocrScaleFactor).rounded())

    matchIndices.append(Int64(matchIndices.count))
    texts.append(text)
    confidences.append(Double(candidate.confidence))
    xValues.append(x)
    yValues.append(y)
    widthValues.append(width)
    heightValues.append(height)
  }

  return NativeOcrTextResponse(
    recognized_at: nativeNowIso8601(),
    image_path: imagePath.intoRustString(),
    image_width: Int64(image.width),
    image_height: Int64(image.height),
    query: nativeSanitize(rawQuery).intoRustString(),
    exact: request.exact,
    case_sensitive: request.case_sensitive,
    normalized_query: normalizedAnchorQuery.intoRustString(),
    crop_enabled: request.crop_enabled,
    crop_x: cropOffsetX,
    crop_y: cropOffsetY,
    crop_width: request.crop_width,
    crop_height: request.crop_height,
    ocr_scale_factor: Double(ocrScaleFactor),
    match_indices: nativeVec(matchIndices),
    texts: nativeStringVec(texts),
    confidences: nativeVec(confidences),
    x_values: nativeVec(xValues),
    y_values: nativeVec(yValues),
    width_values: nativeVec(widthValues),
    height_values: nativeVec(heightValues),
    error_message: nil,
    recovery_hint: nil
  )
}

func find_visual_rows(request: NativeVisualRowsRequest) -> NativeVisualRowsResponse {
  let imagePath = request.image_path.toString()
  guard let image = loadImage(path: imagePath) else {
    return emptyVisualRowsResponse(
      request: request,
      message: "could not load image for visual row detection at \(imagePath)",
      recovery: "verify the image artifact path exists and is readable"
    )
  }

  var workingImage = image
  var cropOffsetX: Int64 = 0
  var cropOffsetY: Int64 = 0

  if request.crop_enabled {
    let cropRect = CGRect(
      x: Int(request.crop_x),
      y: Int(request.crop_y),
      width: Int(request.crop_width),
      height: Int(request.crop_height)
    ).integral
    guard cropRect.width > 0, cropRect.height > 0 else {
      return emptyVisualRowsResponse(
        request: request,
        message: "invalid visual-row crop rect \(cropRect)",
        recovery: "pass a positive crop region inside the image bounds"
      )
    }
    guard let croppedImage = image.cropping(to: cropRect) else {
      return emptyVisualRowsResponse(
        request: request,
        message: "could not crop visual-row image to \(cropRect)",
        recovery: "pass a crop region inside the image bounds"
      )
    }
    workingImage = croppedImage
    cropOffsetX = Int64(cropRect.origin.x.rounded())
    cropOffsetY = Int64(cropRect.origin.y.rounded())
  }

  guard
    let colorSpace = CGColorSpace(name: CGColorSpace.sRGB),
    let context = CGContext(
      data: nil,
      width: workingImage.width,
      height: workingImage.height,
      bitsPerComponent: 8,
      bytesPerRow: 0,
      space: colorSpace,
      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    )
  else {
    return emptyVisualRowsResponse(
      request: request,
      message: "could not allocate bitmap context for visual row detection",
      recovery: "retry with a standard PNG screenshot artifact"
    )
  }

  context.draw(workingImage, in: CGRect(x: 0, y: 0, width: workingImage.width, height: workingImage.height))

  guard let rawData = context.data else {
    return emptyVisualRowsResponse(
      request: request,
      message: "visual row bitmap context did not expose pixel data",
      recovery: "retry with a standard PNG screenshot artifact"
    )
  }

  let width = workingImage.width
  let height = workingImage.height
  let bytesPerRow = context.bytesPerRow
  let pixels = rawData.bindMemory(to: UInt8.self, capacity: bytesPerRow * height)

  func pixelComponents(x: Int, y: Int) -> (r: Double, g: Double, b: Double, a: Double) {
    let offset = y * bytesPerRow + x * 4
    return (
      Double(pixels[offset]) / 255.0,
      Double(pixels[offset + 1]) / 255.0,
      Double(pixels[offset + 2]) / 255.0,
      Double(pixels[offset + 3]) / 255.0
    )
  }

  func luminance(_ pixel: (r: Double, g: Double, b: Double, a: Double)) -> Double {
    0.2126 * pixel.r + 0.7152 * pixel.g + 0.0722 * pixel.b
  }

  func saturation(_ pixel: (r: Double, g: Double, b: Double, a: Double)) -> Double {
    let maxValue = max(pixel.r, max(pixel.g, pixel.b))
    let minValue = min(pixel.r, min(pixel.g, pixel.b))
    return maxValue - minValue
  }

  func edgeStrength(x: Int, y: Int) -> Double {
    let pixel = pixelComponents(x: x, y: y)
    let rightPixel = pixelComponents(x: min(width - 1, x + 1), y: y)
    let downPixel = pixelComponents(x: x, y: min(height - 1, y + 1))
    return
      abs(luminance(pixel) - luminance(rightPixel))
      + abs(luminance(pixel) - luminance(downPixel))
  }

  func isActive(x: Int, y: Int) -> Bool {
    let pixel = pixelComponents(x: x, y: y)
    guard pixel.a > 0.05 else { return false }
    return edgeStrength(x: x, y: y) >= 0.10 || saturation(pixel) > 0.24
  }

  let stripLeft = max(0, Int((Double(width) * 0.02).rounded()))
  let stripRight = min(width, max(stripLeft + 1, Int((Double(width) * 0.24).rounded())))
  let stripWidth = max(1, stripRight - stripLeft)

  var rowSignal = Array(repeating: 0.0, count: height)
  for y in 0..<height {
    var activeCount = 0
    for x in stripLeft..<stripRight {
      if isActive(x: x, y: y) {
        activeCount += 1
      }
    }
    rowSignal[y] = Double(activeCount) / Double(stripWidth)
  }

  let smoothingRadius = 2
  var smoothedSignal = Array(repeating: 0.0, count: height)
  for y in 0..<height {
    let lower = max(0, y - smoothingRadius)
    let upper = min(height - 1, y + smoothingRadius)
    let window = rowSignal[lower...upper]
    smoothedSignal[y] = window.reduce(0.0, +) / Double(window.count)
  }

  let rowThreshold = 0.018
  let maxGap = 10
  let minBandHeight = 28
  let maxBandHeight = 220

  var rawBands = [(start: Int, end: Int)]()
  var bandStart: Int? = nil
  var gap = 0
  for y in 0..<height {
    if smoothedSignal[y] >= rowThreshold {
      if bandStart == nil {
        bandStart = y
      }
      gap = 0
    } else if let start = bandStart {
      gap += 1
      if gap > maxGap {
        rawBands.append((start: start, end: y - gap))
        bandStart = nil
        gap = 0
      }
    }
  }
  if let start = bandStart {
    rawBands.append((start: start, end: height - 1))
  }

  let filteredBands = rawBands.filter { band in
    let bandHeight = band.end - band.start + 1
    return bandHeight >= minBandHeight && bandHeight <= maxBandHeight
  }

  var rowIndices: [Int64] = []
  var xValues: [Int64] = []
  var yValues: [Int64] = []
  var widthValues: [Int64] = []
  var heightValues: [Int64] = []
  var peakDensities: [Double] = []

  for (bandIndex, band) in filteredBands.enumerated() {
    let bandTop = max(0, band.start - 6)
    let bandBottom = min(height - 1, band.end + 6)
    let bandHeight = bandBottom - bandTop + 1

    var leftX: Int?
    var rightX: Int?
    for x in 0..<width {
      var activeCount = 0
      for y in bandTop...bandBottom {
        if isActive(x: x, y: y) {
          activeCount += 1
        }
      }
      let columnDensity = Double(activeCount) / Double(max(1, bandHeight))
      if columnDensity >= 0.04 {
        if leftX == nil {
          leftX = x
        }
        rightX = x
      }
    }

    let visualLeft = max(0, (leftX ?? stripLeft) - 8)
    let visualRight = min(width - 1, (rightX ?? (width - 1)) + 8)
    let visualWidth = max(1, visualRight - visualLeft + 1)
    let peakDensity = smoothedSignal[band.start...band.end].max() ?? 0.0

    rowIndices.append(Int64(bandIndex))
    xValues.append(cropOffsetX + Int64(visualLeft))
    yValues.append(cropOffsetY + Int64(bandTop))
    widthValues.append(Int64(visualWidth))
    heightValues.append(Int64(bandHeight))
    peakDensities.append(peakDensity)
  }

  return NativeVisualRowsResponse(
    detected_at: nativeNowIso8601(),
    image_path: imagePath.intoRustString(),
    image_width: Int64(image.width),
    image_height: Int64(image.height),
    crop_enabled: request.crop_enabled,
    crop_x: cropOffsetX,
    crop_y: cropOffsetY,
    crop_width: request.crop_width,
    crop_height: request.crop_height,
    analysis_strip_x: Int64(stripLeft),
    analysis_strip_y: 0,
    analysis_strip_width: Int64(stripWidth),
    analysis_strip_height: Int64(height),
    row_indices: nativeVec(rowIndices),
    x_values: nativeVec(xValues),
    y_values: nativeVec(yValues),
    width_values: nativeVec(widthValues),
    height_values: nativeVec(heightValues),
    peak_densities: nativeVec(peakDensities),
    error_message: nil,
    recovery_hint: nil
  )
}
