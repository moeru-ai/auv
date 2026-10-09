import type { CSSProperties } from 'react'

import type { SceneState } from '../scene/types'
import type { Theme } from '../theme'
import type { Fit, Veil } from './DeskGL'

import { useEffect, useLayoutEffect, useRef } from 'react'

import { DeskGL } from './DeskGL'

interface Props {
  className?: string
  fit: Fit
  focus?: null | string
  /** Cap on drawing-buffer pixels; the default keeps big HiDPI screens near 4K. */
  maxPixels?: number
  stage?: { h: number, w: number }
  state: SceneState
  style?: CSSProperties
  theme: Theme
  veil?: Veil
}

/**
 * Canvas that renders the scene synchronously during commit, so a
 * `flushSync` state update means the frame is drawn when it returns.
 */
export function GLDesk({ className, fit, focus, maxPixels, stage, state, style, theme, veil }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const glRef = useRef<DeskGL | null>(null)

  // Created lazily by the layout effect below; disposed on unmount.
  useEffect(() => () => {
    glRef.current?.dispose()
    glRef.current = null
  }, [])

  useLayoutEffect(() => {
    const canvas = canvasRef.current!
    let gl = glRef.current
    if (!gl) {
      gl = new DeskGL(canvas)
      glRef.current = gl
    }
    gl.setSize(canvas.clientWidth, canvas.clientHeight, devicePixelRatio, maxPixels)
    gl.render(state, theme, { fit, focus, stage, veil })
  })

  return <canvas className={className ?? 'gl-desk'} ref={canvasRef} style={style} />
}
