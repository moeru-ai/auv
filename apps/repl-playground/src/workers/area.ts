import type { Area, AreaEdges, AreaLength, AreaSource, Point, Rect } from '../script-api/api'

import { contains } from '@auv-js/sdk'

/**
 * Script-side implementation of `Area`: an immutable screen-space rectangle
 * derived from handles or other areas. It crosses to the host as a plain
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
    return this.#derive(this.x, this.y - gap - height, this.width, height)
  }

  at(fx: number, fy: number): Point {
    return { x: this.x + this.width * fx, y: this.y + this.height * fy }
  }

  below(height: number, gap = 0): Area {
    return this.#derive(this.x, this.y + this.height + gap, this.width, height)
  }

  contains(target: Point | Rect): boolean {
    return contains(this, target)
  }

  inset(by: number | Partial<Record<'bottom' | 'left' | 'right' | 'top', number>>): Area {
    const { bottom = 0, left = 0, right = 0, top = 0 } = typeof by === 'number' ? { bottom: by, left: by, right: by, top: by } : by
    return this.#derive(this.x + left, this.y + top, this.width - left - right, this.height - top - bottom)
  }

  leftOf(width: number, gap = 0): Area {
    return this.#derive(this.x - gap - width, this.y, width, this.height)
  }

  named(label: string): Area {
    return new ScreenArea(this.x, this.y, this.width, this.height, label, this.from)
  }

  offset(dx: number, dy: number): Area {
    return this.#derive(this.x + dx, this.y + dy, this.width, this.height)
  }

  region(edges: AreaEdges): Area {
    const [x, width] = axis('left', edges.left, edges.right, edges.width, this.width)
    const [y, height] = axis('top', edges.top, edges.bottom, edges.height, this.height)
    return this.#derive(this.x + x, this.y + y, width, height)
  }

  rightOf(width: number, gap = 0): Area {
    return this.#derive(this.x + this.width + gap, this.y, width, this.height)
  }

  /** Derived areas keep where they came from but not the parent's label. */
  #derive(x: number, y: number, width: number, height: number): Area {
    return new ScreenArea(x, y, width, height, undefined, this.from)
  }
}

/** Starts an area from a handle, an OCR match, a rectangle or another area. */
export function areaOf(source: AreaSource, label?: string): Area {
  const object = source as unknown as Record<string, unknown>
  const from = typeof object.$ref === 'string' ? object.$ref : typeof object.from === 'string' ? object.from : undefined
  const rect = (isRect(object.frame) ? object.frame : isRect(object.bounds) ? object.bounds : object) as Rect
  if (!isRect(rect))
    throw new TypeError('area() expects a window, display, frame, OCR match or { x, y, width, height }')
  const inherited = typeof object.label === 'string' && object.kind === 'area' ? object.label : undefined
  return new ScreenArea(rect.x, rect.y, rect.width, rect.height, label ?? inherited, from)
}

/**
 * Resolves one axis of `region()` to `[offset, size]` inside `total`. Two of
 * start/end/size pin the box; a missing start is 0 and a missing size fills
 * the rest. All three at once is ambiguous and rejected.
 */
function axis(start: string, startValue?: AreaLength, endValue?: AreaLength, sizeValue?: AreaLength, total = 0): [number, number] {
  if (startValue !== undefined && endValue !== undefined && sizeValue !== undefined)
    throw new RangeError(`region(): ${start === 'left' ? 'left, right and width' : 'top, bottom and height'} over-constrain the box; give two of them`)
  const from = length(startValue, total)
  const to = length(endValue, total)
  const size = length(sizeValue, total)
  if (size !== undefined)
    return [from ?? (to === undefined ? 0 : total - to - size), size]
  const offset = from ?? 0
  return [offset, total - offset - (to ?? 0)]
}

function isRect(value: unknown): value is Rect {
  const rect = value as null | Partial<Rect> | undefined
  return typeof rect?.x === 'number' && typeof rect.y === 'number' && typeof rect.width === 'number' && typeof rect.height === 'number'
}

function length(value: AreaLength | undefined, total: number): number | undefined {
  if (value === undefined)
    return undefined
  if (typeof value === 'number')
    return value
  const percent = Number.parseFloat(value)
  if (!value.endsWith('%') || Number.isNaN(percent))
    throw new TypeError(`Expected a number or a percentage such as '25%', got ${JSON.stringify(value)}`)
  return total * percent / 100
}
