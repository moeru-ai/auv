import { thumbHashToRGBA } from 'thumbhash'

// Captures arrive as metadata with a ThumbHash (`CapturedFrame.thumbhash`)
// before their pixels. Frames show the blurred preview at once and fade the
// pixels in when they load, like nolebase's `NolebaseUnlazyImg`.

/**
 * NOTICE(preview-fade): the fade lasts 750 ms, the duration
 * `NolebaseUnlazyImg` (nolebase/integrations, vitepress-plugin-thumbnail-hash)
 * uses for the same transition.
 */
export const REVEAL_MS = 750

/** Pixels that may still be loading. */
export interface RevealedImage {
  /** The decoded pixels, once loaded. */
  bitmap?: ImageBitmap
  /** The ThumbHash preview, shown until `bitmap` has faded in. */
  preview?: ImageBitmap
  /**
   * `performance.now()` when `bitmap` arrived over a visible preview. Absent
   * means `bitmap` shows at once, e.g. when it loaded before any preview.
   */
  revealedAt?: number
}

/** Decodes a ThumbHash into a tiny bitmap; drawing it enlarged blurs it. */
export async function decodeThumbHash(hash: Uint8Array | undefined): Promise<ImageBitmap | undefined> {
  if (!hash || hash.length === 0)
    return undefined
  try {
    const { h, rgba, w } = thumbHashToRGBA(hash)
    return await createImageBitmap(new ImageData(new Uint8ClampedArray(rgba), w, h))
  }
  catch (error) {
    console.warn('Decoding a capture ThumbHash failed', error)
    return undefined
  }
}

/**
 * Draws `image` into a rectangle: the preview, with the pixels faded in over
 * it. Returns whether the fade is still running, so the caller keeps drawing.
 */
export function paintRevealed(ctx: CanvasRenderingContext2D, image: RevealedImage, x: number, y: number, width: number, height: number, now = performance.now()): boolean {
  const progress = revealProgress(image, now)
  if (image.preview && progress < 1)
    ctx.drawImage(image.preview, x, y, width, height)
  if (image.bitmap && progress > 0) {
    const alpha = ctx.globalAlpha
    ctx.globalAlpha = alpha * progress
    ctx.drawImage(image.bitmap, x, y, width, height)
    ctx.globalAlpha = alpha
  }
  return image.bitmap !== undefined && progress < 1
}

/** Opacity of the pixels over the preview at `now`: 0 before they load, eased to 1. */
export function revealProgress(image: RevealedImage, now: number): number {
  if (!image.bitmap)
    return 0
  if (image.revealedAt === undefined)
    return 1
  const t = Math.min(1, Math.max(0, (now - image.revealedAt) / REVEAL_MS))
  // Ease in and out, like CSS `ease-in-out`.
  return t < 0.5 ? 4 * t * t * t : 1 - (-2 * t + 2) ** 3 / 2
}

/** Whether a newly loaded bitmap should fade in: only over a preview already on screen. */
export function revealStart(image: RevealedImage): number | undefined {
  return image.preview ? performance.now() : undefined
}
