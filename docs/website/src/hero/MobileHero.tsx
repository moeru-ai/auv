// Mobile counterpart of the desktop HeroIcon: the same three unfolds (A ->
// ghost cursor, U -> app windows, V -> line pointer + REPL), but each one is a
// colored copy of its part that flies up out of the gray mark into the
// showcase area, and back when another part is picked.

import type { CSSProperties } from 'react'

import type { Theme } from '../theme'

import { clamp } from 'es-toolkit'

import { alpha, mixOklab } from '../lib/color'
import { easeOutBack, lerp } from '../lib/ease'
import { GHOST_HOTSPOT, ghostTemplate, ICON_CENTER, PART_PATHS, partTemplates } from '../lib/icon'
import { morphPts, toPath, transform } from '../lib/shape'
import { partSlab, rectSlab, slabBox, slabPath, windowToU } from '../lib/slab'
import { glassStyle } from '../render/glass'
import { COLORFUL, textWidth } from '../render/Overlay'
import { SANS, WindowChromeImpl } from '../render/WindowContent'
import { GHOSTS } from '../theme'
import { lineCenter, replAt, ReplBody } from './HeroIcon'

const seg = (h: number, a: number, b: number) => Math.max(0, (h - a) / (b - a))
const V_CENTER = { x: 214.75, y: 141.8 }

interface Props {
  /** Showcase area above the mark. */
  area: { h: number, w: number, x: number, y: number }
  /** 0..1 fade for scrolling away; applied per element (wrappers would break the glass). */
  fade: number
  /** Spring progress per part (A, U, V), 0 = folded in the mark. */
  h: [number, number, number]
  /** The gray mark: center and scale (icon units -> px). */
  mark: { s: number, x: number, y: number }
  t: number
  theme: Theme
}

export function MobileHero({ area, fade, h, mark, t, theme }: Props) {
  const [hA, hU, hV] = h
  const cx = area.x + area.w / 2
  const cy = area.y + area.h / 2
  const placed = (i: number) => transform(partTemplates()[i], mark.s, mark.x - ICON_CENTER.x * mark.s, mark.y - ICON_CENTER.y * mark.s)
  const o = 1 - fade

  // A: a big ghost cursor with what it is doing.
  const ghostH = Math.min(area.w * 0.34, 150)
  const gs = ghostH / 20
  const tip = { x: cx - ghostH * 0.35, y: cy - ghostH * 0.55 }
  const ghost = transform(ghostTemplate(), gs, tip.x - GHOST_HOTSPOT.x * gs, tip.y - GHOST_HOTSPOT.y * gs)
  const aPts = morphPts(placed(0), ghost, clamp(hA, 0, 1), [partTemplates()[0], ghostTemplate()])
  const pulse = (t % 1.3) / 1.3

  // U: front window and two back windows.
  const fw = Math.min(area.w * 0.86, 440)
  const fh = fw * 0.68
  const win = { h: fh, w: fw, x: cx - fw / 2, y: cy - fh / 2 }
  const front = windowToU(
    rectSlab(win.x, win.y, win.w, win.h, 20),
    partSlab(mark.s, mark.x, mark.y),
    1 - seg(hU, 0.3, 1),
    clamp(1 - seg(hU, 0.12, 0.6), 0, 1),
    clamp(1 - seg(hU, 0, 0.45), 0, 1),
  )
  const fbox = slabBox(front)
  const fpath = slabPath(front, fbox.x, fbox.y)
  const backs = clamp(seg(hU, 0.45, 1), 0, 1)
  const back = (dx: number, dy: number, rot: number, tint: string): CSSProperties => ({
    borderRadius: 20,
    height: win.h,
    left: win.x,
    top: win.y,
    width: win.w,
    ...glassStyle(theme, false, mixOklab(theme.tint, tint, 0.14)),
    boxShadow: `inset 0 0 0 1px ${theme.edge}`,
    opacity: backs * o,
    transform: `translate(${dx * backs}px, ${dy * backs}px) rotate(${rot * backs}deg) scale(${0.72 + 0.22 * backs})`,
  })
  const uFill = mixOklab(mixOklab(theme.icon[1], COLORFUL[1], clamp(hU * 3, 0, 1)), theme.tint, clamp(seg(hU, 0.2, 0.7), 0, 1))

  // V: the REPL window springs out of the pointer; V becomes the pointer.
  const rw = Math.min(area.w, 460)
  const rh = (rw * 330) / 600
  const repl = { h: rh, w: rw, x: cx - rw / 2, y: cy - rh / 2 }
  const k = rw / 600
  const run = replAt(t)
  const fromY = lineCenter(Math.max(0, run.step - 1))
  const toY = lineCenter(run.step)
  const lineY = run.run < 0 ? lineCenter(0) : lerp(fromY, toY, easeOutBack(clamp(run.since / 0.24, 0, 1), 2.4))
  const gutter = { x: repl.x + 22 * k, y: repl.y + lineY * k }
  const replScale = seg(hV, 0.15, 1)
  const vRest = { x: mark.x + (V_CENTER.x - ICON_CENTER.x) * mark.s, y: mark.y + (V_CENTER.y - ICON_CENTER.y) * mark.s }
  const vMove = clamp(seg(hV, 0, 0.8), 0, 1)
  const vPos = { x: lerp(vRest.x, gutter.x, vMove), y: lerp(vRest.y, gutter.y, vMove) }
  const vScale = lerp(mark.s, 0.2 * k, clamp(seg(hV, 0.05, 0.7), 0, 1))

  const label = 'clicking “Send” for you'
  const lw = textWidth(label, 15) + 30

  return (
    <>
      {backs > 0.01 && (
        <>
          <div className="win" style={back(-30, -26, -5, GHOSTS.cyan.fill)} />
          <div className="win" style={back(34, -40, 5, GHOSTS.pink.fill)} />
        </>
      )}
      {hU > 0.005 && (
        <div className="win" style={{ clipPath: `path('${fpath}')`, height: fbox.h, left: fbox.x, top: fbox.y, width: fbox.w, ...glassStyle(theme, false, uFill), opacity: o }}>
          <svg height={fbox.h} style={{ inset: 0, position: 'absolute' }} width={fbox.w}>
            <path d={fpath} fill="none" stroke={alpha(theme.edge, clamp(seg(hU, 0.3, 0.8), 0, 1))} strokeWidth={1.5} />
            {hU > 0.7 && (
              <g opacity={clamp(seg(hU, 0.75, 1), 0, 1)} transform={`translate(${win.x - fbox.x} ${win.y - fbox.y})`}>
                <WindowChromeImpl
                  clipId="mh-browser-clip"
                  theme={theme}
                  win={{ accent: 'violet', chrome: 1, data: { scroll: (Math.sin(t * 0.8) * 0.5 + 0.5) * 100 }, id: 'mh-browser', kind: 'browser', opacity: 1, rect: { h: win.h, w: win.w, x: 0, y: 0 }, title: 'Browser', z: 0 }}
                />
              </g>
            )}
          </svg>
        </div>
      )}
      {replScale > 0.01 && (
        <div
          className="win"
          style={{
            borderRadius: 18,
            height: repl.h,
            left: repl.x,
            top: repl.y,
            width: repl.w,
            ...glassStyle(theme, true),
            boxShadow: `inset 0 0 0 1px ${theme.edge}`,
            opacity: clamp(replScale * 3, 0, 1) * o,
            transform: `scale(${replScale})`,
            transformOrigin: `${gutter.x - repl.x}px ${gutter.y - repl.y}px`,
          }}
        >
          <ReplBody fluid run={run} theme={theme} w={600} />
        </div>
      )}
      <svg className="m2-mark" height="100%" style={{ opacity: o }} width="100%">
        {hA > 0.005 && (
          <>
            <path d={toPath(aPts)} fill={mixOklab(theme.icon[0], COLORFUL[0], clamp(hA * 3, 0, 1))} stroke={alpha('#ffffff', 0.75 * clamp(hA, 0, 1))} strokeLinejoin="round" strokeWidth={3} />
            {hA > 0.6 && <circle cx={tip.x} cy={tip.y} fill="none" opacity={(1 - pulse) * clamp((hA - 0.6) / 0.4, 0, 1)} r={8 + 40 * pulse} stroke={GHOSTS.cyan.fill} strokeWidth={4 * (1 - pulse)} />}
            {hA > 0.4 && (
              <g transform={`translate(${Math.min(tip.x + ghostH * 0.5, area.x + area.w - lw)} ${tip.y + ghostH * 0.95}) scale(${easeOutBack(clamp((hA - 0.4) / 0.6, 0, 1), 2.2)})`}>
                <rect fill={GHOSTS.cyan.fill} height={34} rx={17} width={lw} />
                <text fill={GHOSTS.cyan.label} fontFamily={SANS} fontSize={15} fontWeight={650} x={15} y={22.5}>{label}</text>
              </g>
            )}
          </>
        )}
        {hV > 0.005 && (
          <g transform={`translate(${vPos.x} ${vPos.y}) rotate(${-90 * hV}) scale(${vScale}) translate(${-V_CENTER.x} ${-V_CENTER.y})`}>
            <path d={PART_PATHS[2]} fill={mixOklab(theme.icon[2], COLORFUL[2], clamp(hV * 3, 0, 1))} />
          </g>
        )}
      </svg>
    </>
  )
}
