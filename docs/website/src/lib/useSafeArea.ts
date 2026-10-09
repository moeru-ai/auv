import { useEffect, useState } from 'react'

/** Reads the `env(safe-area-inset-*)` paddings once, for notched portrait screens. */
export function useSafeArea() {
  const [safe, setSafe] = useState({ bottom: 0, top: 0 })
  useEffect(() => {
    const probe = document.createElement('div')
    probe.style.cssText = 'position:fixed;top:0;left:0;padding-top:env(safe-area-inset-top);padding-bottom:env(safe-area-inset-bottom);visibility:hidden;pointer-events:none'
    document.body.appendChild(probe)
    const cs = getComputedStyle(probe)
    setSafe({ bottom: Number.parseFloat(cs.paddingBottom) || 0, top: Number.parseFloat(cs.paddingTop) || 0 })
    probe.remove()
  }, [])
  return safe
}
