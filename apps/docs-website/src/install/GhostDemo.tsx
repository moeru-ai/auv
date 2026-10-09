// The install section's AUV mark, drawn over a slot in the section. Now and
// then its A (cyan) or V (pink) turns into a ghost cursor, flies to an
// install-method tab, clicks it twice with a label, then flies home and turns
// back into the part. Alternates A and V; pauses while off screen; skipped
// under prefers-reduced-motion.

import type { RefObject } from 'react'

import type { Pt } from '../lib/shape'
import type { Theme } from '../theme'

import { clamp } from 'es-toolkit'
import { useEffect, useRef, useState } from 'react'

import { alpha, mix } from '../lib/color'
import { bump, easeInOutCubic, easeOutBack } from '../lib/ease'
import { GHOST_HOTSPOT, ghostTemplate, ICON_CENTER, PART_PATHS, partTemplates } from '../lib/icon'
import { bbox, morph, placeByHeight, transform } from '../lib/shape'
import { textWidth } from '../render/Overlay'
import { SANS } from '../render/WindowContent'
import { GHOSTS } from '../theme'

// Stable templates, so morph alignment is computed once (it caches per array).
const PARTS = partTemplates()
const GHOST = ghostTemplate()
const GHOST_BOX = bbox(GHOST)
/** Mark viewBox origin, as in the nav mark. */
const VIEW = { h: 92.4, x: ICON_CENTER.x - 110.5, y: 96 }

// Timeline (s): morph out, fly over, two clicks, fly back, morph home.
const T = { clicks: [1.5, 1.82], end: 4.05, fly: 0.9, home: 3.6, leave: 2.7, morph: 0.45 }
const FIRST = 2.2
const EVERY = 5.5

export interface GhostTarget { el: HTMLElement, label: string, onClick?: () => void }

interface Box { h: number, w: number, x: number, y: number }

interface Run {
  clicked: boolean
  label: string
  onClick?: () => void
  part: 0 | 2
  start: number
  tgt: Pt
}

export function GhostDemo({ hidden, mark, pickTarget, section, theme }: {
  hidden: boolean
  mark: RefObject<HTMLElement | null>
  pickTarget: () => GhostTarget | null
  section: RefObject<HTMLElement | null>
  theme: Theme
}) {
  const [box, setBox] = useState<Box | null>(null)
  const [run, setRun] = useState<null | Run>(null)
  const [now, setNow] = useState(0)
  const pickRef = useRef(pickTarget)
  pickRef.current = pickTarget
  const runRef = useRef(run)
  runRef.current = run

  // Where the mark slot sits inside the section.
  useEffect(() => {
    const measure = () => {
      const s = section.current?.getBoundingClientRect()
      const m = mark.current?.getBoundingClientRect()
      if (s && m)
        setBox({ h: m.height, w: m.width, x: m.left - s.left, y: m.top - s.top })
    }
    measure()
    const ro = new ResizeObserver(measure)
    if (section.current)
      ro.observe(section.current)
    return () => ro.disconnect()
  }, [section, mark])

  // Scheduler: start a run when the mark and the target are both on screen.
  useEffect(() => {
    if (hidden || matchMedia('(prefers-reduced-motion: reduce)').matches)
      return
    let part: 0 | 2 = 0
    let timer = window.setTimeout(function tick() {
      const s = section.current?.getBoundingClientRect()
      const m = mark.current?.getBoundingClientRect()
      const target = pickRef.current()
      const r = target?.el.getBoundingClientRect()
      if (!s || !m || !target || !r || runRef.current || m.top < 0 || r.bottom > innerHeight - 80 || document.hidden) {
        timer = window.setTimeout(tick, 1200)
        return
      }
      setRun({ clicked: false, label: target.label, onClick: target.onClick, part, start: performance.now(), tgt: [r.left - s.left + r.width * 0.6, r.top - s.top + r.height * 0.62] })
      part = part === 0 ? 2 : 0
      timer = window.setTimeout(tick, (T.end + EVERY) * 1000)
    }, FIRST * 1000)
    return () => clearTimeout(timer)
  }, [hidden, section, mark])

  // Frame clock, only while a run is on.
  useEffect(() => {
    if (!run)
      return
    let raf = 0
    const loop = () => {
      const t = (performance.now() - run.start) / 1000
      if (t >= T.end) {
        setRun(null)
        return
      }
      if (!run.clicked && t >= T.clicks[0]) {
        run.clicked = true
        run.onClick?.()
      }
      setNow(t)
      raf = requestAnimationFrame(loop)
    }
    raf = requestAnimationFrame(loop)
    return () => cancelAnimationFrame(raf)
  }, [run])

  if (!box || hidden)
    return null

  const s = box.h / VIEW.h
  const tx = box.x - VIEW.x * s
  const ty = box.y - VIEW.y * s
  const away = run ? run.part : -1

  return (
    <svg aria-hidden="true" className="install-ghosts">
      <g transform={`translate(${tx} ${ty}) scale(${s})`}>
        {PART_PATHS.map((d, i) => i !== away && <path d={d} fill={theme.icon[i]} key={i} />)}
      </g>
      {run && <Ghost run={run} s={s} t={now} theme={theme} tx={tx} ty={ty} width={section.current?.clientWidth ?? 0} />}
    </svg>
  )
}

function Ghost({ run, s, t, theme, tx, ty, width }: { run: Run, s: number, t: number, theme: Theme, tx: number, ty: number, width: number }) {
  const color = run.part === 0 ? GHOSTS.cyan : GHOSTS.pink
  const part = transform(PARTS[run.part], s, tx, ty)
  const pb = bbox(part)
  const h = clamp(pb.h * 0.75, 22, 32)
  const ghost = placeByHeight(GHOST, pb.cx, pb.cy, h)
  const k = h / GHOST_BOX.h
  const tip: Pt = [(GHOST_HOTSPOT.x - GHOST_BOX.cx) * k + pb.cx, (GHOST_HOTSPOT.y - GHOST_BOX.cy) * k + pb.cy]

  // Shape: part -> cursor at the start, cursor -> part at the end.
  const m = t < T.home ? easeInOutCubic(clamp(t / T.morph, 0, 1)) : 1 - easeInOutCubic(clamp((t - T.home) / (T.end - T.home), 0, 1))
  // Path: an arc out to the target, a short hold for the clicks, an arc back.
  const arc = (f: number, a: Pt, b: Pt): Pt => {
    const c: Pt = [(a[0] + b[0]) / 2 + 40, Math.min(a[1], b[1]) - 50]
    const u = 1 - f
    return [u * u * a[0] + 2 * u * f * c[0] + f * f * b[0], u * u * a[1] + 2 * u * f * c[1] + f * f * b[1]]
  }
  const out = easeInOutCubic(clamp((t - T.morph) / T.fly, 0, 1))
  const back = easeInOutCubic(clamp((t - T.leave) / (T.home - T.leave), 0, 1))
  const at = t < T.leave ? arc(out, tip, run.tgt) : arc(back, run.tgt, tip)
  const dx = at[0] - tip[0]
  const dy = at[1] - tip[1]
  const press = 1 - 0.16 * Math.max(...T.clicks.map(c => bump(t, c + 0.05, 0.07)))
  const fill = mix(theme.icon[run.part], color.fill, m)

  // Label pops in beside the cursor while it works; flips left near the edge.
  const labelIn = clamp((t - (T.morph + T.fly * 0.7)) / 0.3, 0, 1) * (1 - clamp((t - T.leave) / 0.2, 0, 1))
  const lw = textWidth(run.label, 13) + 24
  const flip = at[0] + 14 + lw > width - 8

  return (
    <>
      {T.clicks.map((c) => {
        const p = (t - c) / 0.55
        return p > 0 && p < 1 && <circle cx={run.tgt[0]} cy={run.tgt[1]} fill="none" key={c} opacity={1 - p} r={4 + 20 * p} stroke={color.fill} strokeWidth={3 * (1 - p)} />
      })}
      <g transform={`translate(${dx} ${dy})`}>
        <g transform={`translate(${tip[0]} ${tip[1]}) scale(${press}) translate(${-tip[0]} ${-tip[1]})`}>
          <path d={morph(part, ghost, m, [PARTS[run.part], GHOST])} fill={fill} stroke={alpha('#ffffff', 0.8 * m)} strokeLinejoin="round" strokeWidth={2} />
        </g>
      </g>
      {labelIn > 0 && (
        <g opacity={labelIn} transform={`translate(${flip ? at[0] - 10 - lw : at[0] + 14} ${at[1] + h * 0.7}) scale(${easeOutBack(labelIn, 2)})`}>
          <rect fill={color.fill} height={26} rx={13} width={lw} />
          <text fill={color.label} fontFamily={SANS} fontSize={13} fontWeight={650} x={12} y={17.5}>{run.label}</text>
        </g>
      )}
    </>
  )
}
