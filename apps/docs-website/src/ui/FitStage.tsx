import type { ReactNode } from 'react'

import { useEffect, useState } from 'react'

/**
 * Fixed-size stage scaled into the viewport. Uses a transform, which does not
 * create a backdrop root, so glass inside still blurs the stage background.
 */
export function FitStage({ children, height, mode, style, width }: { children: ReactNode, height: number, mode: 'cover' | 'meet', style?: React.CSSProperties, width: number }) {
  const { k, x, y } = useFit(width, height, mode)
  return (
    <div className="fit" style={style}>
      <div style={{ height, left: 0, position: 'absolute', top: 0, transform: `translate(${x}px, ${y}px) scale(${k})`, transformOrigin: '0 0', width }}>
        {children}
      </div>
    </div>
  )
}

/** Scale factor and offset that fit a `width x height` stage into the viewport. */
export function useFit(width: number, height: number, mode: 'cover' | 'meet') {
  const { h, w } = useViewport()
  const k = mode === 'meet' ? Math.min(w / width, h / height) : Math.max(w / width, h / height)
  return { k, x: (w - width * k) / 2, y: (h - height * k) / 2 }
}

export function useViewport() {
  const [size, setSize] = useState({ h: innerHeight, w: innerWidth })
  useEffect(() => {
    const onResize = () => setSize({ h: innerHeight, w: innerWidth })
    addEventListener('resize', onResize)
    return () => removeEventListener('resize', onResize)
  }, [])
  return size
}
