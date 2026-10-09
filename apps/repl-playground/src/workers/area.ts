import type { Area, AreaEdges, AreaSource, Point, Rect } from '../script-api/api'

import { above, at, below, contains, inset, leftOf, offset, region, rightOf } from '@auv-js/sdk'

import { refOf } from '../handles'

/**
 * Script-side implementation of `Area`: an immutable screen-space rectangle
 * derived from handles or other areas. The geometry is the SDK's (the same
 * rules as Rust's `Rect`); this class adds chaining, `named()` and the source
 * handle. It crosses to the host as a plain
 * `{ kind: 'area', x, y, width, height, label?, from? }` object.
 */
class ScreenArea implements Area {
  readonly kind = 'area'

  get center(): Point {
    return this.at(0.5, 0.5)
  }

  constructor(
    readonly x: number,
    readonly y: number,
    readonly width: number,
    readonly height: number,
    readonly label?: string,
    readonly from?: string,
  ) {}

  above(height: number, gap = 0): Area {
    return this.#derive(above(this, height, gap))
  }

  at(fx: number, fy: number): Point {
    return at(this, fx, fy)
  }

  below(height: number, gap = 0): Area {
    return this.#derive(below(this, height, gap))
  }

  contains(target: Point | Rect): boolean {
    return contains(this, target)
  }

  inset(by: number | Partial<Record<'bottom' | 'left' | 'right' | 'top', number>>): Area {
    return this.#derive(inset(this, by))
  }

  leftOf(width: number, gap = 0): Area {
    return this.#derive(leftOf(this, width, gap))
  }

  named(label: string): Area {
    return new ScreenArea(this.x, this.y, this.width, this.height, label, this.from)
  }

  offset(dx: number, dy: number): Area {
    return this.#derive(offset(this, dx, dy))
  }

  region(edges: AreaEdges): Area {
    return this.#derive(region(this, edges))
  }

  rightOf(width: number, gap = 0): Area {
    return this.#derive(rightOf(this, width, gap))
  }

  /** Derived areas keep where they came from but not the parent's label. */
  #derive({ height, width, x, y }: Rect): Area {
    return new ScreenArea(x, y, width, height, undefined, this.from)
  }
}

/** Starts an area from a handle, an OCR match, a rectangle or another area. */
export function areaOf(source: AreaSource, label?: string): Area {
  const object = source as unknown as Record<string, unknown>
  const from = refOf(object) ?? (typeof object.from === 'string' ? object.from : undefined)
  // An SDK `WindowClient` keeps its frame on `window`.
  const window = object.window as undefined | { frame?: unknown }
  const rect = [object.frame, window?.frame, object.bounds, object].find(isRect)
  if (!rect)
    throw new TypeError('area() expects a window, display, frame, OCR match or { x, y, width, height }')
  const inherited = typeof object.label === 'string' && object.kind === 'area' ? object.label : undefined
  return new ScreenArea(rect.x, rect.y, rect.width, rect.height, label ?? inherited, from)
}

function isRect(value: unknown): value is Rect {
  const rect = value as null | Partial<Rect> | undefined
  return typeof rect?.x === 'number' && typeof rect.y === 'number' && typeof rect.width === 'number' && typeof rect.height === 'number'
}
