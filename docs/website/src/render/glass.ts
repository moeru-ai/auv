import type { CSSProperties } from 'react'

import type { Theme } from '../theme'

/** CSS glass for DOM elements layered over the WebGL desk (hero, info cards). */
export function glassStyle(theme: Theme, dark: boolean, fill?: string): CSSProperties {
  return {
    backdropFilter: theme.blur,
    background: fill ?? (dark ? theme.termTint : theme.tint),
    WebkitBackdropFilter: theme.blur,
  }
}
