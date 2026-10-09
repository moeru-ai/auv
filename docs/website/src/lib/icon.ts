// AUV mark geometry, copied from the macOS helper icon
// (auv/crates/auv-device-helper-macos/package/AUV Helper.icon/Assets/*.svg, 285x285 box).
//
//   part 1  "A"  upward rounded triangle   -> becomes a ghost cursor
//   part 2  "U"  sheared rounded slab       -> becomes app windows
//   part 3  "V"  downward rounded triangle  -> becomes a REPL / script
//
// Fill colors follow icon.json: light appearance uses the SVG grays, dark
// appearance uses the "dark" fill specializations.

import type { Pt } from './shape'

import { roundedRect, samplePath, transform } from './shape'

export const PART_PATHS = [
  'M66.0727 102.798C68.7703 96.4007 78.2297 96.4006 80.9272 102.798L112.409 177.46C114.535 182.5 110.66 188 104.982 188H42.0177C36.3401 188 32.4649 182.5 34.5905 177.46L66.0727 102.798Z',
  'M97.4346 109.203C94.6354 101.911 98.6982 96 106.509 96H149.755C155.616 96 162.069 100.435 164.169 105.906L181.679 151.522C189.502 171.903 178.147 188.424 156.316 188.424V188.424C140.592 188.424 123.276 176.524 117.641 161.844L97.4346 109.203Z',
  'M221.927 181.202C219.23 187.599 209.77 187.599 207.073 181.202L175.591 106.54C173.465 101.5 177.34 96 183.018 96L245.982 96C251.66 96 255.535 101.5 253.409 106.54L221.927 181.202Z',
] as const

export const ICON_COLORS = {
  dark: ['#FFFFFF', '#AEAEAE', '#5E5E5E'],
  light: ['#242424', '#4A4A4A', '#797979'],
} as const

/** Visual center of the three-part mark inside the 285 box. */
export const ICON_CENTER = { x: 144, y: 142.2 }
export const ICON_BOX = { h: 92.4, w: 221, x: 34, y: 96 }

/** AUV overlay ghost cursor (auv/crates/auv-driver-overlay-common/assets/cursor-auv.svg). */
export const GHOST_PATH = 'M3 2 Q0 0 0 4 L0 20 Q0 24 3 21.5 L8 17 Q9.5 15.5 12 15.5 L16 15.5 Q20 15.5 17 12.5 Z'
export const GHOST_HOTSPOT = { x: 1, y: 1.5 }

/** Classic system arrow pointer, drawn black with a white keyline. */
export const USER_PATH = 'M1.5 1 L1.5 18.6 L5.6 14.7 L8.4 21.2 L11.4 19.9 L8.7 13.6 L14.4 13.6 Z'
export const USER_HOTSPOT = { x: 1.5, y: 1 }

export const partTemplates = () => PART_PATHS.map(d => samplePath(d))
export const ghostTemplate = () => samplePath(GHOST_PATH)
/** Canonical rect, only used as a stable alignment reference for morphs. */
export const RECT_TEMPLATE: Pt[] = roundedRect(0, 0, 160, 110, 14)

/** Ghost cursor outline with its hotspot at (x, y). */
export function placedGhost(x: number, y: number, s: number) {
  return transform(ghostTemplate(), s, x - GHOST_HOTSPOT.x * s, y - GHOST_HOTSPOT.y * s)
}

/** Icon part outline placed in stage space: scale `s`, mark centered on (cx, cy). */
export function placedPart(i: number, s: number, cx: number, cy: number) {
  return transform(partTemplates()[i], s, cx - ICON_CENTER.x * s, cy - ICON_CENTER.y * s)
}
