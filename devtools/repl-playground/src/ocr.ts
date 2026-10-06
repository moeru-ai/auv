/**
 * Maps OCR confidence to an opacity for text and boxes.
 *
 * NOTICE(ocr-confidence-curve): recognizer confidences cluster near 1, so a
 * linear 0..1 mapping would make everything look equally solid. Values at or
 * below 0.5 floor at 0.3 opacity, and the 0.5..1 range spreads over 0.3..1.
 */
export function confidenceAlpha(confidence: number): number {
  const t = Math.min(1, Math.max(0, (confidence - 0.5) / 0.5))
  return 0.3 + 0.7 * t
}

export function confidenceLabel(confidence: number): string {
  return `${Math.round(confidence * 100)}% confidence`
}
