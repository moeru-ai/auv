import { ICON_COLORS } from './lib/icon'

export interface Theme {
  bg: string
  blobOpacity: number
  blobs: string[]
  blur: string
  /** Opening display line-up: card fill at its center, its rim, and the lit desk display's glass. */
  display: string
  /**
   * How much of the line-up behind the desk display still shows through once
   * it lights up. The defocused cards stack into one haze behind its glass.
   */
  displayBehindLit: number
  displayLit: string
  displayRim: string
  /** Mark parts once docked into their displays: a faint gray. */
  docked: string
  edge: string
  glass: string
  glassEdge: string
  glassTitle: string
  /** Peak alpha of per-window glows. */
  glow: number
  hairline: string
  highlight: string
  icon: readonly string[]
  ink: string
  ink2: string
  ink3: string
  name: ThemeName
  shadow: string
  skeleton: string
  term: string
  termInk: string
  termTint: string
  /** Backdrop-filter glass. */
  tint: string
}

export type ThemeName = 'dark' | 'light'

export const THEMES: Record<ThemeName, Theme> = {
  dark: {
    bg: '#0c0e13',
    blobOpacity: 0.3,
    blobs: ['#0d4f57', '#5b1c45', '#30266e', '#28451a', '#5a3712'],
    blur: 'blur(30px) saturate(1.7)',
    display: 'rgba(176,190,230,0.17)',
    displayBehindLit: 1,
    displayLit: 'rgba(150,165,215,0.12)',
    displayRim: 'rgba(214,224,255,0.12)',
    docked: 'rgba(205,210,228,0.36)',
    edge: 'rgba(255,255,255,0.13)',
    glass: 'rgba(38,41,51,0.66)',
    glassEdge: 'rgba(255,255,255,0.14)',
    glassTitle: 'rgba(255,255,255,0.04)',
    glow: 0.26,
    hairline: 'rgba(255,255,255,0.07)',
    highlight: 'rgba(255,255,255,0.22)',
    icon: ICON_COLORS.dark,
    ink: '#eceae4',
    ink2: '#a5a8b3',
    ink3: '#6c707c',
    name: 'dark',
    shadow: 'rgba(0,0,0,0.45)',
    skeleton: 'rgba(255,255,255,0.08)',
    term: 'rgba(10,11,15,0.86)',
    termInk: '#e8e6df',
    termTint: 'rgba(10,11,16,0.58)',
    tint: 'rgba(36,39,50,0.40)',
  },
  light: {
    bg: '#f6f7fb',
    blobOpacity: 0.32,
    blobs: ['#aeeef2', '#ffcfe4', '#d9cfff', '#dcf5c2', '#ffe2bd'],
    blur: 'blur(30px) saturate(1.4) brightness(1.04)',
    display: 'rgba(52,62,104,0.13)',
    // NOTICE: on the light desk the stacked cards read as a dirty blue-gray
    // smudge behind the windows, so they mostly recede as the display lights up.
    displayBehindLit: 0.25,
    displayLit: 'rgba(255,255,255,0.55)',
    displayRim: 'rgba(52,62,104,0.1)',
    docked: 'rgba(70,76,96,0.34)',
    edge: 'rgba(255,255,255,0.75)',
    glass: 'rgba(255,255,255,0.74)',
    glassEdge: 'rgba(255,255,255,0.95)',
    glassTitle: 'rgba(255,255,255,0.45)',
    glow: 0.18,
    hairline: 'rgba(24,28,40,0.08)',
    highlight: 'rgba(255,255,255,0.95)',
    icon: ICON_COLORS.light,
    ink: '#1b1d23',
    ink2: '#5d6271',
    ink3: '#9aa0ae',
    name: 'light',
    shadow: 'rgba(40,50,90,0.16)',
    skeleton: 'rgba(24,28,44,0.085)',
    term: 'rgba(22,24,31,0.92)',
    termInk: '#e8e6df',
    termTint: 'rgba(22,24,32,0.74)',
    tint: 'rgba(255,255,255,0.46)',
  },
}

/** Ghost cursor palette. Each agent keeps its own color, as AUV overlay themes allow. */
export const GHOSTS = {
  amber: { edge: '#fff0d4', fill: '#ffb238', label: '#4a2e00' },
  cyan: { edge: '#cefffd', fill: '#3fd9dd', label: '#0b3a3c' },
  lime: { edge: '#ecffd9', fill: '#86d94a', label: '#1d3a08' },
  pink: { edge: '#ffe1ee', fill: '#ff74b1', label: '#ffffff' },
  violet: { edge: '#ebe4ff', fill: '#9b7dff', label: '#ffffff' },
} as const

export type GhostColor = keyof typeof GHOSTS

export function currentTheme(): ThemeName {
  const q = new URLSearchParams(location.search).get('theme')
  if (q === 'light' || q === 'dark')
    return q
  return matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
}
