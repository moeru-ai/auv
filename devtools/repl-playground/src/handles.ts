import type { Point, Rect } from './script-api/api'
import type { Resource } from './store'

/** Short human hint for a handle, so a list of refs is readable without hovering. */
export function refHint(resource: Resource | undefined): string | undefined {
  switch (resource?.kind) {
    case 'display':
      return resource.handle.name
    case 'frame':
      return `${resource.handle.width}×${resource.handle.height}`
    case 'input':
      return resource.handle.action
    case 'text':
      if (resource.handle.matches.length === 0)
        return 'no match'
      return `${resource.handle.matches.length} match${resource.handle.matches.length === 1 ? '' : 'es'}`
    case 'window':
      return resource.handle.app || resource.handle.title || undefined
  }
  return undefined
}

// NOTICE(focus-point-neighbourhood): a point has no size; focusing one shows
// this much of the desktop around it (logical points).
const POINT_VIEW = { height: 150, width: 240 }

/**
 * Screen-space rectangle of anything positional a script passes to
 * `focus()`: handles by ref (windows, displays, frames, OCR results, clicks),
 * areas and rectangles, OCR matches (`bounds`), or points.
 */
export function boundsOf(value: unknown, resources: Record<string, Resource>): Rect | undefined {
  const object = value as null | Record<string, unknown> | undefined
  if (!object || typeof object !== 'object')
    return undefined
  const resource = typeof object.$ref === 'string' ? resources[object.$ref] : undefined
  switch (resource?.kind) {
    case 'display':
    case 'window':
      return resource.handle.frame
    case 'frame':
      return resource.handle.bounds
    case 'input':
      return resource.handle.point ? around(resource.handle.point) : undefined
    case 'text': {
      const rects = resource.handle.matches.map(match => match.bounds)
      if (rects.length > 0)
        return union(rects)
      return resource.handle.within ?? resource.handle.frame?.bounds
    }
  }
  if (isRect(object))
    return { height: object.height, width: object.width, x: object.x, y: object.y }
  if (isRect(object.bounds))
    return object.bounds
  if (typeof object.x === 'number' && typeof object.y === 'number')
    return around({ x: object.x, y: object.y })
  return undefined
}

function around(point: Point): Rect {
  return { ...POINT_VIEW, x: point.x - POINT_VIEW.width / 2, y: point.y - POINT_VIEW.height / 2 }
}

function isRect(value: unknown): value is Rect {
  const rect = value as null | Partial<Rect> | undefined
  return typeof rect?.x === 'number' && typeof rect.y === 'number' && typeof rect.width === 'number' && typeof rect.height === 'number'
}

function union(rects: Rect[]): Rect {
  const x = Math.min(...rects.map(rect => rect.x))
  const y = Math.min(...rects.map(rect => rect.y))
  const right = Math.max(...rects.map(rect => rect.x + rect.width))
  const bottom = Math.max(...rects.map(rect => rect.y + rect.height))
  return { height: bottom - y, width: right - x, x, y }
}
