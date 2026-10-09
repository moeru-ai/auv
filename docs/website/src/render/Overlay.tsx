// Crisp, un-blurred top layer: cursors, labels, click ripples, cursor -> icon
// morphs, and the final mark. Drawn in front-view stage coordinates.

import type { CursorNode, IconNode, LabelState, Ripple } from '../scene/types'
import type { Theme } from '../theme'

import { alpha, mixOklab } from '../lib/color'
import { GHOST_HOTSPOT, GHOST_PATH, ICON_CENTER, PART_PATHS, USER_HOTSPOT, USER_PATH } from '../lib/icon'
import { GHOSTS } from '../theme'
import { SANS } from './WindowContent'

export const GHOST_SCALE = 2.1
export const USER_SCALE = 2.1

interface Props {
  cursors: CursorNode[]
  height: number
  icon?: IconNode
  id: string
  ripples: Ripple[]
  theme: Theme
  width: number
}

export function Overlay({ cursors, height, icon, id, ripples, theme, width }: Props) {
  const ordered = cursors.slice().sort((a, b) => (a.kind === b.kind ? 0 : a.kind === 'user' ? -1 : 1))
  return (
    <svg className="overlay" height={height} viewBox={`0 0 ${width} ${height}`} width={width}>
      <defs>
        <filter height="200%" id={`${id}-cshadow`} width="200%" x="-50%" y="-50%">
          <feDropShadow dx="0" dy="2" floodColor="#000" floodOpacity="0.28" stdDeviation="2.4" />
        </filter>
      </defs>
      {ripples.map((r, i) => (
        <g key={i} opacity={1 - r.p}>
          <circle cx={r.x} cy={r.y} fill="none" r={8 + 46 * r.p} stroke={r.color} strokeWidth={4 * (1 - r.p) + 1} />
          <circle cx={r.x} cy={r.y} fill={alpha(r.color, 0.35)} r={14 * (1 - r.p)} />
        </g>
      ))}
      {ordered.map(c => <CursorView c={c} id={id} key={c.id} stageW={width} />)}
      {icon && <IconView icon={icon} theme={theme} />}
    </svg>
  )
}

/** Approximate advance of SANS 15px semibold; SVG text cannot be measured off-DOM. */
export function textWidth(s: string, size = 15) {
  let w = 0
  for (const ch of s)
    w += /[A-Z]/.test(ch) ? 10.2 : /[ .,'·]/.test(ch) ? 4.6 : /[\u3000-\u9FFF]/.test(ch) ? 15 : 8.3
  return (w * size) / 15
}

function CursorView({ c, id, stageW }: { c: CursorNode, id: string, stageW: number }) {
  if (c.opacity <= 0.001)
    return null
  if (c.outline)
    return <path d={c.outline} fill={c.outlineFill} opacity={c.opacity} />
  const g = GHOSTS[c.color]
  const ghost = c.kind === 'ghost'
  const s = (ghost ? GHOST_SCALE : USER_SCALE) * c.scale
  const hot = ghost ? GHOST_HOTSPOT : USER_HOTSPOT
  return (
    <g opacity={c.opacity}>
      {c.label && ghost && <Label fill={g.fill} ink={g.label} label={c.label} stageW={stageW} x={c.x} y={c.y} />}
      <g filter={`url(#${id}-cshadow)`} transform={`translate(${c.x} ${c.y}) scale(${s}) translate(${-hot.x} ${-hot.y})`}>
        {ghost
          ? <path d={GHOST_PATH} fill={g.fill} stroke={g.edge} strokeLinejoin="round" strokeWidth={1.6} />
          : <path d={USER_PATH} fill="#111" stroke="#fff" strokeLinejoin="round" strokeWidth={1.3} />}
      </g>
    </g>
  )
}

function Label({ fill, ink, label, stageW, x, y }: { fill: string, ink: string, label: LabelState, stageW: number, x: number, y: number }) {
  if (label.pop <= 0.001)
    return null
  const text = label.text.slice(0, label.chars)
  const w = 30 + textWidth(text)
  // Flip to the cursor's left when the pill would run off the stage (as DeskGL does).
  const flip = x + 26 + w > stageW - 8
  const lx = flip ? x - 14 - w * label.pop : x + 26
  return (
    <g opacity={Math.min(1, label.pop * 1.5)} transform={`translate(${lx} ${y + 40}) scale(${label.pop})`}>
      <rect fill={fill} height={32} rx={16} width={w} x={0} y={0} />
      <rect fill="none" height={32} rx={16} stroke="rgba(255,255,255,0.55)" width={w} x={0} y={0} />
      <text fill={ink} fontFamily={SANS} fontSize={15} fontWeight={600} x={15} y={21}>{text}</text>
    </g>
  )
}

export const COLORFUL = [GHOSTS.cyan.fill, GHOSTS.violet.fill, GHOSTS.pink.fill]

export function IconView({ icon, theme }: { icon: IconNode, theme: Theme }) {
  return (
    <g opacity={icon.opacity} transform={`translate(${icon.cx - ICON_CENTER.x * icon.s} ${icon.cy - ICON_CENTER.y * icon.s}) scale(${icon.s})`}>
      {PART_PATHS.map((d, i) => (
        <path d={d} fill={mixOklab(theme.icon[i], COLORFUL[i], typeof icon.color === 'number' ? icon.color : icon.color[i])} key={i} />
      ))}
    </g>
  )
}
