// The interactive AUV mark, drawn in film stage coordinates so the landing can
// hand off from the film's last frame without a jump. Each part unfolds into
// the capability it stands for:
//
//   A -> a ghost cursor that clicks for you
//   U -> the mark's slab un-rounds, straightens, and grows into app windows
//   V -> turns counter-clockwise into a debugger's line pointer; a REPL window
//        springs out of its top-left corner and the pointer steps through code
//
// NOTICE: REPL snippets are provisional illustrations of the SDK style
// (`findText`, `click`, `type`, `capture`), not a frozen API.

import type { CSSProperties } from 'react'

import type { Theme } from '../theme'

import { memo, useEffect, useRef, useState } from 'react'

import { alpha, mix } from '../lib/color'
import { clamp, easeOutBack, lerp } from '../lib/ease'
import { ghostTemplate, ICON_CENTER, PART_PATHS, partTemplates, placedPart } from '../lib/icon'
import { bbox, morph, placeByHeight } from '../lib/shape'
import { partSlab, rectSlab, slabBox, slabPath, windowToU } from '../lib/slab'
import { glassStyle } from '../render/glass'
import { COLORFUL, textWidth } from '../render/Overlay'
import { MONO, SANS, WindowChrome } from '../render/WindowContent'
import { FILM_ICON } from '../scene/film'
import { GHOSTS } from '../theme'

export const PART_NAMES = ['Ghost cursors', 'App windows', 'REPL'] as const

interface Box { h: number, w: number, x: number, y: number }
const within = (b: Box, x: number, y: number) => x >= b.x && x <= b.x + b.w && y >= b.y && y <= b.y + b.h
const pad = (b: Box, m: number): Box => ({ h: b.h + 2 * m, w: b.w + 2 * m, x: b.x - m, y: b.y - m })
/** Stage progress of `h` across [a, b]; open-ended above so spring overshoot carries through. */
const seg = (h: number, a: number, b: number) => Math.max(0, (h - a) / (b - a))

/** Underdamped springs integrated per frame; interactive, so frame-rate dependence is fine here. */
function useSprings(targets: number[]) {
  const state = useRef(targets.map(() => ({ v: 0, vel: 0 })))
  const target = useRef(targets)
  target.current = targets
  const [time, setTime] = useState(0)
  useEffect(() => {
    let raf = 0
    let last = performance.now()
    const loop = (now: number) => {
      const dt = Math.min(0.033, (now - last) / 1000)
      last = now
      let moving = false
      state.current.forEach((s, i) => {
        const acc = 170 * (target.current[i] - s.v) - 19 * s.vel
        s.vel += acc * dt
        s.v += s.vel * dt
        if (Math.abs(s.vel) > 1e-3 || Math.abs(target.current[i] - s.v) > 1e-3 || target.current[i] !== 0)
          moving = true
      })
      // Idle and fully folded: skip the re-render. Hover changes props and wakes it.
      if (moving)
        setTime(t => t + dt)
      raf = requestAnimationFrame(loop)
    }
    raf = requestAnimationFrame(loop)
    return () => cancelAnimationFrame(raf)
  }, [])
  return [state.current.map(s => s.v), time] as const
}

const PROGRAMS = [
  {
    lines: ['const notes = await auv.app(\'Notes\')', 'await notes.findText(\'New\').click()', 'await notes.type(\'Ship the landing\')', 'const shot = await notes.capture()'],
    results: ['→ AppHandle', '✓ clicked · 14ms', '✓ typed · 22ms', '→ CaptureRef #7'],
  },
  {
    lines: ['const music = await auv.app(\'Music\')', 'await music.findText(\'Lo-fi\').click()', 'await music.press(\'Space\')', 'const frame = await music.capture()'],
    results: ['→ AppHandle', '✓ clicked · 11ms', '✓ playing', '→ CaptureRef #8'],
  },
]
const TYPE = 0.7
const STEP = 0.78
const DONE = 1.0
const CLEAR = 0.45
const CYCLE = TYPE + STEP * 4 + DONE + CLEAR
export const LINE_H = 38
export const CODE_TOP = 72
/** Vertical center of code line i; text, highlight band, and pointer all align to it. */
export const lineCenter = (i: number) => CODE_TOP + i * LINE_H
/** Baseline offset that centers 16px monospace caps/x-height on the line center. */
const BASELINE = 5.5

interface Props {
  /** 0..1: recede while something else on the page has focus. */
  dim?: number
  onHover?: (part: null | number) => void
  /** Pointer in stage coordinates, or null when outside the page. */
  pointer: null | { x: number, y: number }
  /** Icon scale in stage units; defaults to the film's final size. */
  scale?: number
  theme: Theme
}

/** Where the REPL is in its run loop at time `t`. */
export function replAt(t: number) {
  const cycle = Math.floor(t / CYCLE)
  const lt = t - cycle * CYCLE
  const program = PROGRAMS[cycle % PROGRAMS.length]
  const run = lt - TYPE
  const step = run < 0 ? 0 : Math.min(4, Math.floor(run / STEP))
  const since = run < 0 ? 1 : run - step * STEP
  const typed = clamp(lt / TYPE)
  const clearing = clamp((lt - (TYPE + STEP * 4 + DONE)) / CLEAR)
  return { clearing, program, run, since, step, typed }
}

/** Memoized so the landing's per-frame clock does not re-render it; it runs its own spring loop. */
export const HeroIcon = memo(HeroIconImpl)

export function ReplBody({ fluid = false, run, theme, w }: { fluid?: boolean, run: ReturnType<typeof replAt>, theme: Theme, w: number }) {
  const { clearing, program, run: rt, step, typed } = run
  const total = program.lines.join('').length
  let budget = Math.floor(typed * total)
  const done = rt >= STEP * 4
  return (
    <svg {...(fluid ? { height: '100%', viewBox: `0 0 ${w} 330`, width: '100%' } : { height: 330, width: w })} style={{ inset: 0, position: 'absolute' }}>
      {['#ff5f57', '#febc2e', '#28c840'].map((c, i) => <circle cx={22 + i * 20} cy={21} fill={c} key={c} r={6} />)}
      <text fill="#8b8f9b" fontFamily={SANS} fontSize={13} fontWeight={600} textAnchor="middle" x={w / 2} y={26}>auv repl</text>
      <line stroke="rgba(255,255,255,0.08)" x1={0} x2={w} y1={42} y2={42} />
      <g opacity={1 - clearing}>
        {rt >= 0 && step < 4 && <rect fill={alpha(GHOSTS.pink.fill, 0.14)} height={32} rx={8} width={w - 20} x={10} y={lineCenter(step) - 16} />}
        {program.lines.map((line, i) => {
          const n = Math.max(0, Math.min(line.length, budget))
          budget -= n
          const ran = rt >= 0 && (i < step || done)
          return (
            <g key={i} opacity={ran ? 0.55 : 1} transform={`translate(44 ${lineCenter(i) + BASELINE})`}>
              <text fill={theme.termInk} fontFamily={MONO} fontSize={16}>
                {tokens(line.slice(0, n)).map((tk, k) => <tspan fill={tk.c} key={k}>{tk.s}</tspan>)}
              </text>
              {ran && <ResultChip text={program.results[i]} x={w - 64} />}
            </g>
          )
        })}
        {done && (
          <text fill="#86d94a" fontFamily={MONO} fontSize={15} x={44} y={lineCenter(4) + BASELINE}>✓ 4 steps · run recorded · replayable</text>
        )}
      </g>
    </svg>
  )
}

function HeroIconImpl({ dim = 0, onHover, pointer, scale = FILM_ICON.s, theme }: Props) {
  const hoverRef = useRef<null | number>(null)
  const [hover, setHover] = useState<null | number>(null)
  const [[hA, hU, hV, hAny], time] = useSprings([hover === 0 ? 1 : 0, hover === 1 ? 1 : 0, hover === 2 ? 1 : 0, hover !== null ? 1 : 0])
  const { cx, cy } = FILM_ICON
  const s = scale
  const fade = 1 - 0.55 * dim
  const toStage = (x: number, y: number) => ({ x: (x - ICON_CENTER.x) * s + cx, y: (y - ICON_CENTER.y) * s + cy })

  // Neighbors make room for whichever part unfolds. When the REPL opens, A and
  // U slide left so the mark and the window sit centered as one group.
  const REPL_W = 600
  const GAP = 36
  const restURight = toStage(183.6, 0).x
  const groupW = restURight - toStage(34, 0).x + GAP + REPL_W
  const leftEdge = cx - groupW / 2
  const shiftAU = (leftEdge + (restURight - toStage(34, 0).x) - restURight) * hV
  const push = [shiftAU - 230 * hU, shiftAU + 70 * hA, 230 * hU + 70 * hA]
  const parts = [0, 1, 2].map(i => placedPart(i, s, cx + push[i], cy))
  const boxes = parts.map(p => bbox(p))

  // A: ghost cursor.
  const ghost = placeByHeight(ghostTemplate(), boxes[0].cx - 6, boxes[0].cy - 4, boxes[0].h * 0.98)
  const tip = ghost.reduce((a, b) => (b[0] + b[1] < a[0] + a[1] ? b : a))

  // U: windows.
  const uSlab = partSlab(s, cx + push[1], cy)
  const winRect = { h: 370, w: 540, x: cx + push[1] - 270, y: cy - 200 }
  const front = windowToU(
    rectSlab(winRect.x, winRect.y, winRect.w, winRect.h, 22),
    uSlab,
    1 - seg(hU, 0.3, 1),
    clamp(1 - seg(hU, 0.12, 0.6)),
    clamp(1 - seg(hU, 0, 0.45)),
  )
  const frontBox = slabBox(front)
  const frontD = slabPath(front, frontBox.x, frontBox.y)
  const restU = mix(theme.icon[1], COLORFUL[1], clamp(hAny))
  const frontFill = mix(restU, theme.tint, clamp(seg(hU, 0.2, 0.7)))
  const backs = clamp(seg(hU, 0.45, 1))

  // V: the REPL window springs out of V's top-left corner (its scale origin)
  // and settles to the right of the mark; V becomes the line pointer.
  const corner = toStage(175, 96)
  const repl = { h: 330, w: REPL_W, x: leftEdge + (restURight - toStage(34, 0).x) + GAP, y: cy - 165 }
  const origin = { x: corner.x - repl.x, y: corner.y - repl.y }
  const replScale = seg(hV, 0.12, 1)
  const run = replAt(time)
  const fromY = lineCenter(Math.max(0, run.step - 1))
  const toY = lineCenter(run.step)
  const pointerLocal = { x: 24, y: run.run < 0 ? lineCenter(0) : lerp(fromY, toY, easeOutBack(clamp(run.since / 0.24), 2.4)) }
  const toRepl = (p: { x: number, y: number }) => ({ x: repl.x + origin.x + (p.x - origin.x) * replScale, y: repl.y + origin.y + (p.y - origin.y) * replScale })
  const gutter = toRepl(pointerLocal)
  const vRest = toStage(214.75, 141.8)
  const vMove = clamp(seg(hV, 0.3, 0.95))
  const vPos = { x: lerp(vRest.x + push[2], gutter.x, vMove), y: lerp(vRest.y, gutter.y, vMove) }
  // Counter-clockwise quarter turn: the down arrow becomes a right-pointing line marker.
  const vRot = -90 * hV
  const vScale = s * lerp(1, 0.12, clamp(seg(hV, 0.15, 0.75)))

  // Hit zones are horizontal bands split at U's current extent: everything left
  // of U is A, everything right of it is V. Parts move as their neighbours
  // unfold, so per-shape boxes made the pointer fall off a part the moment it
  // opened (A slides back right when U folds). Bands keep a sweep continuous.
  // Only U's front window counts once it is open; its back windows are decoration.
  const engaged = hover !== null
  const iconTop = Math.min(...boxes.map(b => b.y))
  const iconBottom = Math.max(...boxes.map(b => b.y + b.h))
  // NOTICE: engaged bands stop at +-520 from center (enough to cover A pushed
  // left by an open U) so they never swallow the desk windows around the edges.
  const bandY = engaged ? { y0: cy - 260, y1: cy + 220 } : { y0: iconTop - 30, y1: iconBottom + 30 }
  const uZone = hover === 1 ? pad(winRect, 6) : pad(boxes[1], 18)
  const aLeft = engaged ? cx - 520 : boxes[0].x - 60
  const vRight = engaged ? Math.max(cx + 520, repl.x + repl.w * clamp(replScale) + 40) : boxes[2].x + boxes[2].w + 60
  const zones: Box[] = [
    { h: bandY.y1 - bandY.y0, w: uZone.x - aLeft, x: aLeft, y: bandY.y0 },
    uZone,
    { h: bandY.y1 - bandY.y0, w: vRight - (uZone.x + uZone.w), x: uZone.x + uZone.w, y: bandY.y0 },
  ]
  // NOTICE: runs after every render on purpose: the zones move with the
  // springs, so hover is re-tested each frame; the ref guard stops the loop.
  // oxlint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => {
    let next: null | number = null
    if (pointer)
      next = [1, 0, 2].find(i => within(zones[i], pointer.x, pointer.y)) ?? null
    if (next !== hoverRef.current) {
      hoverRef.current = next
      setHover(next)
      onHover?.(next)
    }
  })

  const colorA = mix(theme.icon[0], COLORFUL[0], clamp(Math.max(hAny, hA)))
  const colorV = mix(theme.icon[2], COLORFUL[2], clamp(Math.max(hAny, hV)))
  const backStyle = (dx: number, dy: number, rot: number, tint: string): CSSProperties => ({
    borderRadius: 22,
    height: winRect.h,
    left: winRect.x,
    top: winRect.y,
    width: winRect.w,
    ...glassStyle(theme, false, mix(theme.tint, tint, 0.12)),
    boxShadow: `inset 0 0 0 1px ${theme.edge}`,
    opacity: backs,
    transform: `translate(${dx * backs}px, ${dy * backs}px) rotate(${rot * backs}deg) scale(${0.7 + 0.24 * backs})`,
  })

  return (
    <>
      {backs > 0.01 && (
        <>
          <div className="win" style={backStyle(-120, -70, -4, GHOSTS.cyan.fill)} />
          <div className="win" style={backStyle(130, -96, 5, GHOSTS.pink.fill)} />
        </>
      )}
      <div className="win" style={{ clipPath: `path('${frontD}')`, height: frontBox.h, left: frontBox.x, top: frontBox.y, width: frontBox.w, ...glassStyle(theme, false, frontFill), opacity: fade }}>
        <svg height={frontBox.h} style={{ inset: 0, position: 'absolute' }} width={frontBox.w}>
          <path d={frontD} fill="none" stroke={alpha(theme.edge, clamp(seg(hU, 0.3, 0.8)))} strokeWidth={2} />
          {hU > 0.7 && (
            <g opacity={clamp(seg(hU, 0.75, 1))} transform={`translate(${winRect.x - frontBox.x} ${winRect.y - frontBox.y})`}>
              <WindowChrome
                clipId="h-browser-clip"
                theme={theme}
                win={{ accent: 'violet', chrome: 1, data: { scroll: (Math.sin(time * 0.8) * 0.5 + 0.5) * 120 }, id: 'h-browser', kind: 'browser', opacity: 1, rect: winRect, title: 'Browser', z: 0 }}
              />
            </g>
          )}
        </svg>
      </div>

      {replScale > 0.01 && (
        <div className="win" style={{ borderRadius: 22, height: repl.h, left: repl.x, top: repl.y, width: repl.w, ...glassStyle(theme, true), boxShadow: `inset 0 0 0 1px ${theme.edge}`, opacity: clamp(replScale * 3), transform: `scale(${replScale})`, transformOrigin: `${origin.x}px ${origin.y}px` }}>
          <ReplBody run={run} theme={theme} w={repl.w} />
        </div>
      )}

      <svg className="overlay" height={1080} style={{ opacity: fade }} viewBox="0 0 1920 1080" width={1920}>
        <path d={morph(parts[0], ghost, clamp(hA), [partTemplates()[0], ghostTemplate()])} fill={colorA} stroke={alpha('#ffffff', 0.75 * clamp(hA))} strokeLinejoin="round" strokeWidth={4} />
        {hA > 0.4 && <HeroLabel color="cyan" p={(hA - 0.4) / 0.6} text="clicking “Send” for you" x={tip[0] + 90} y={tip[1] + 150} />}
        {hA > 0.6 && <LoopRipple o={clamp((hA - 0.6) / 0.4)} t={time} x={tip[0]} y={tip[1]} />}
        <g transform={`translate(${vPos.x} ${vPos.y}) rotate(${vRot}) scale(${vScale}) translate(${-214.75} ${-141.8})`}>
          <path d={PART_PATHS[2]} fill={colorV} />
        </g>
      </svg>
    </>
  )
}

function HeroLabel({ color, p, text, x, y }: { color: keyof typeof GHOSTS, p: number, text: string, x: number, y: number }) {
  const pop = easeOutBack(clamp(p), 2.2)
  const w = textWidth(text, 20) + 40
  return (
    <g transform={`translate(${x} ${y}) scale(${pop})`}>
      <rect fill={GHOSTS[color].fill} height={44} rx={22} width={w} />
      <text fill={GHOSTS[color].label} fontFamily={SANS} fontSize={20} fontWeight={650} x={20} y={29}>{text}</text>
    </g>
  )
}

function LoopRipple({ o, t, x, y }: { o: number, t: number, x: number, y: number }) {
  const p = (t % 1.3) / 1.3
  return <circle cx={x} cy={y} fill="none" opacity={(1 - p) * o} r={8 + 46 * p} stroke={GHOSTS.cyan.fill} strokeWidth={5 * (1 - p)} />
}

function ResultChip({ text, x }: { text: string, x: number }) {
  // Monospace: one advance per character (12px SF Mono is ~7.2px wide).
  const tw = [...text].length * 7.3 + 18
  return (
    <g transform={`translate(${x - tw} ${-BASELINE - 11})`}>
      <rect fill="rgba(255,255,255,0.08)" height={22} rx={11} width={tw} />
      <text fill="#b9bdc8" fontFamily={MONO} fontSize={12} x={9} y={15}>{text}</text>
    </g>
  )
}

function tokens(text: string) {
  return text.split(/('[^']*'?|\bawait\b|\bconst\b|auv)/).filter(Boolean).map((s) => {
    if (s.startsWith('\''))
      return { c: '#a6e37d', s }
    if (s === 'await' || s === 'const')
      return { c: '#c4a8ff', s }
    if (s === 'auv')
      return { c: '#6ee7ea', s }
    return { c: '#e8e6df', s }
  })
}
