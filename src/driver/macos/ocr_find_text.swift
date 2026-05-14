import Foundation
import Vision
import ImageIO

let imagePath = __IMAGE_PATH__
let rawQuery = __QUERY__
let exact = __EXACT__
let caseSensitive = __CASE_SENSITIVE__
let maxObservations = __MAX_OBSERVATIONS__

func sanitize(_ raw: String) -> String {
  raw
    .replacingOccurrences(of: "\t", with: " ")
    .replacingOccurrences(of: "\n", with: " ")
    .replacingOccurrences(of: "\r", with: " ")
    .trimmingCharacters(in: .whitespacesAndNewlines)
}

let imageURL = URL(fileURLWithPath: imagePath)
guard
  let imageSource = CGImageSourceCreateWithURL(imageURL as CFURL, nil),
  let image = CGImageSourceCreateImageAtIndex(imageSource, 0, nil)
else {
  fputs("could not load image for OCR at \(imagePath)\n", stderr)
  exit(1)
}

let normalizedQuery = caseSensitive ? rawQuery : rawQuery.lowercased()

func matches(_ text: String) -> Bool {
  let normalizedText = caseSensitive ? text : text.lowercased()
  if exact {
    return normalizedText == normalizedQuery
  }
  return normalizedText.contains(normalizedQuery)
}

let request = VNRecognizeTextRequest()
request.recognitionLevel = .accurate
request.usesLanguageCorrection = false

let handler = VNImageRequestHandler(cgImage: image, options: [:])
do {
  try handler.perform([request])
} catch {
  fputs("vision OCR failed: \(error)\n", stderr)
  exit(1)
}

let observations = (request.results as? [VNRecognizedTextObservation]) ?? []

print("recognizedAt=\(ISO8601DateFormatter().string(from: Date()))")
print("imagePath=\(imagePath)")
print("imageWidth=\(image.width)")
print("imageHeight=\(image.height)")
print("query=\(sanitize(rawQuery))")
print("exact=\(exact ? "true" : "false")")
print("caseSensitive=\(caseSensitive ? "true" : "false")")

var matchCount = 0
for observation in observations.prefix(maxObservations) {
  guard let candidate = observation.topCandidates(1).first else { continue }
  let text = sanitize(candidate.string)
  guard !text.isEmpty else { continue }
  guard matches(text) else { continue }

  let boundingBox = observation.boundingBox
  let x = Int((boundingBox.minX * CGFloat(image.width)).rounded())
  let y = Int(((1.0 - boundingBox.maxY) * CGFloat(image.height)).rounded())
  let width = Int((boundingBox.width * CGFloat(image.width)).rounded())
  let height = Int((boundingBox.height * CGFloat(image.height)).rounded())

  print(
    "match\t\(matchCount)\t\(text)\t\(String(format: "%.6f", candidate.confidence))\t\(x)\t\(y)\t\(width)\t\(height)"
  )
  matchCount += 1
}

print("matchCount=\(matchCount)")
