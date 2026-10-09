// Vector skeletons of macOS-style app windows, drawn in window-local space.
// They should read as "real apps" at a glance without becoming UI mockups.

import type { WindowData, WindowNode } from '../scene/types'
import type { Theme } from '../theme'

import { clamp } from 'es-toolkit'
import { memo } from 'react'

import { alpha, mix } from '../lib/color'
import { easeOutBack } from '../lib/ease'
import { GHOSTS } from '../theme'

export const TITLE_H = 40
export const SANS = '-apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI", "Helvetica Neue", sans-serif'
export const MONO = '"SF Mono", ui-monospace, Menlo, Consolas, monospace'

interface Props { clipId: string, theme: Theme, win: WindowNode }

export function WindowChromeImpl({ clipId, theme, win }: Props) {
  const { h, w } = win.rect
  const dark = win.kind === 'term'
  const accent = GHOSTS[win.accent].fill
  return (
    <g>
      <clipPath id={clipId}>
        <rect height={h} rx={18} width={w} x={0} y={0} />
      </clipPath>
      <g clipPath={`url(#${clipId})`}>
        <rect fill={dark ? 'rgba(255,255,255,0.04)' : theme.glassTitle} height={TITLE_H} width={w} x={0} y={0} />
        <line stroke={dark ? 'rgba(255,255,255,0.07)' : theme.hairline} x1={0} x2={w} y1={TITLE_H} y2={TITLE_H} />
        <Content accent={accent} theme={theme} win={win} />
      </g>
      {['#ff5f57', '#febc2e', '#28c840'].map((c, i) => (
        <circle cx={20 + i * 20} cy={TITLE_H / 2} fill={c} key={c} r={6} />
      ))}
      <text
        fill={dark ? '#8b8f9b' : theme.ink2}
        fontFamily={SANS}
        fontSize={13}
        fontWeight={600}
        opacity={win.kind === 'browser' ? 0 : 1}
        textAnchor="middle"
        x={w / 2}
        y={TITLE_H / 2 + 4.5}
      >
        {win.title}
      </text>
    </g>
  )
}

function sameData(a: WindowData, b: WindowData) {
  const ka = Object.keys(a) as (keyof WindowData)[]
  return ka.length === Object.keys(b).length && ka.every(k => a[k] === b[k])
}

/**
 * Window content only depends on size, identity, and `data`, not on position,
 * so a dragged or floating window does not re-render its SVG every frame.
 */
export const WindowChrome = memo(WindowChromeImpl, (a, b) =>
  a.theme === b.theme
  && a.clipId === b.clipId
  && a.win.kind === b.win.kind
  && a.win.title === b.win.title
  && a.win.accent === b.win.accent
  && a.win.rect.w === b.win.rect.w
  && a.win.rect.h === b.win.rect.h
  && sameData(a.win.data, b.win.data))

function Bar({ fill, h = 9, w, x, y }: { fill: string, h?: number, w: number, x: number, y: number }) {
  return <rect fill={fill} height={h} rx={h / 2} width={Math.max(0, w)} x={x} y={y} />
}

function Browser({ accent, theme, win }: { accent: string, theme: Theme, win: WindowNode }) {
  const { h, w } = win.rect
  const scroll = win.data.scroll ?? 0
  const cw = (w - 40 - 32) / 3
  const thumbs = [GHOSTS.cyan.fill, GHOSTS.pink.fill, GHOSTS.violet.fill, GHOSTS.lime.fill, GHOSTS.amber.fill, GHOSTS.pink.fill, GHOSTS.violet.fill, GHOSTS.pink.fill, GHOSTS.amber.fill]
  const gid = `bh-${win.id}`
  return (
    <g>
      <rect fill={theme.skeleton} height={22} rx={8} width={340} x={w / 2 - 170} y={9} />
      <text fill={theme.ink2} fontFamily={SANS} fontSize={12} textAnchor="middle" x={w / 2} y={24.5}>auv.moeru.ai</text>
      <clipPath id={`${gid}-clip`}>
        <rect height={h - TITLE_H} width={w} x={0} y={TITLE_H} />
      </clipPath>
      <g clipPath={`url(#${gid}-clip)`}>
        <g transform={`translate(0 ${-scroll})`}>
          <defs>
            <linearGradient id={gid} x1="0" x2="1" y1="0" y2="1">
              <stop offset="0" stopColor={mix(accent, '#ffffff', 0.25)} />
              <stop offset="0.55" stopColor={mix(GHOSTS.violet.fill, '#ffffff', 0.3)} />
              <stop offset="1" stopColor={mix(GHOSTS.pink.fill, '#ffffff', 0.3)} />
            </linearGradient>
          </defs>
          <rect fill={`url(#${gid})`} height={180} rx={14} width={w - 40} x={20} y={TITLE_H + 18} />
          <text fill="#ffffff" fontFamily={SANS} fontSize={34} fontWeight={800} x={48} y={TITLE_H + 92}>Hello, apps.</text>
          <Bar fill="rgba(255,255,255,0.7)" h={10} w={260} x={48} y={TITLE_H + 116} />
          <Bar fill="rgba(255,255,255,0.5)" h={10} w={190} x={48} y={TITLE_H + 136} />
          {thumbs.map((c, i) => {
            const col = i % 3
            const row = Math.floor(i / 3)
            const x = 20 + col * (cw + 16)
            const y = TITLE_H + 218 + row * 150
            return (
              <g key={i}>
                <rect fill={theme.skeleton} height={134} opacity={0.6} rx={12} width={cw} x={x} y={y} />
                <rect fill={alpha(c, 0.55)} height={78} rx={12} width={cw} x={x} y={y} />
                <Bar fill={theme.skeleton} h={8} w={cw * 0.7} x={x + 12} y={y + 92} />
                <Bar fill={theme.skeleton} h={8} w={cw * 0.45} x={x + 12} y={y + 110} />
              </g>
            )
          })}
        </g>
      </g>
      <rect fill={theme.ink3} height={90} opacity={scroll > 0 ? 0.5 : 0} rx={2} width={4} x={w - 8} y={TITLE_H + 10 + scroll * 0.35} />
      <rect fill="none" height={1} width={w} x={0} y={h - 1} />
    </g>
  )
}

function Content({ accent, theme, win }: { accent: string, theme: Theme, win: WindowNode }) {
  switch (win.kind) {
    case 'browser': return <Browser accent={accent} theme={theme} win={win} />
    case 'chart': return <Chart accent={accent} theme={theme} win={win} />
    case 'music': return <Music accent={accent} theme={theme} win={win} />
    case 'notes': return <Notes accent={accent} theme={theme} win={win} />
    case 'scan': return <Scan accent={accent} theme={theme} win={win} />
    case 'term': return <Term accent={accent} theme={theme} win={win} />
    case 'todo': return <Todo accent={accent} theme={theme} win={win} />
    case 'trace': return <Trace accent={accent} theme={theme} win={win} />
  }
}

function Music({ accent, theme, win }: { accent: string, theme: Theme, win: WindowNode }) {
  const { w } = win.rect
  const playing = win.data.playing ?? -1
  const on = playing >= 0
  const progress = 0.22 + Math.max(0, playing) * 0.025
  const gid = `ma-${win.id}`
  return (
    <g>
      <defs>
        <linearGradient id={gid} x1="0" x2="1" y1="0" y2="1">
          <stop offset="0" stopColor={GHOSTS.pink.fill} />
          <stop offset="1" stopColor={GHOSTS.violet.fill} />
        </linearGradient>
      </defs>
      <rect fill={`url(#${gid})`} height={130} rx={14} width={130} x={24} y={TITLE_H + 20} />
      <circle cx={89} cy={TITLE_H + 85} fill="rgba(255,255,255,0.25)" r={34} />
      <circle cx={89} cy={TITLE_H + 85} fill="rgba(255,255,255,0.8)" r={8} />
      <text fill={theme.ink} fontFamily={SANS} fontSize={18} fontWeight={700} x={176} y={TITLE_H + 48}>Lo-fi for Agents</text>
      <text fill={theme.ink2} fontFamily={SANS} fontSize={13} x={176} y={TITLE_H + 70}>Moeru · Side A</text>
      {Array.from({ length: 12 }, (_, i) => {
        const hgt = on ? 6 + Math.abs(Math.sin(playing * 7 + i * 1.3)) * 22 : 4
        return <rect fill={alpha(accent, 0.85)} height={hgt} key={i} rx={3} width={6} x={176 + i * 10} y={TITLE_H + 120 - hgt} />
      })}
      <Bar fill={theme.skeleton} h={5} w={w - 200} x={176} y={TITLE_H + 140} />
      <Bar fill={accent} h={5} w={(w - 200) * Math.min(progress, 1)} x={176} y={TITLE_H + 140} />
      <g transform={`translate(${w / 2} 270)`}>
        <path d="M38 -8 L48 0 L38 8 Z M48 -8 L58 0 L48 8 Z" fill={theme.ink2} transform="scale(-1 1)" />
        <circle fill={accent} r={22} />
        {on
          ? (
              <g fill="#fff">
                <rect height={16} rx={1.5} width={5} x={-7} y={-8} />
                <rect height={16} rx={1.5} width={5} x={2} y={-8} />
              </g>
            )
          : <path d="M-5 -9 L9 0 L-5 9 Z" fill="#fff" />}
        <path d="M38 -8 L48 0 L38 8 Z M48 -8 L58 0 L48 8 Z" fill={theme.ink2} />
      </g>
    </g>
  )
}

function Notes({ accent, theme, win }: { accent: string, theme: Theme, win: WindowNode }) {
  const { h, w } = win.rect
  const side = 150
  return (
    <g>
      <rect fill={theme.skeleton} height={h - TITLE_H} opacity={0.5} width={side} x={0} y={TITLE_H} />
      {[0, 1, 2, 3, 4].map(i => (
        <g key={i}>
          {i === 0 && <rect fill={alpha(accent, 0.28)} height={30} rx={8} width={side - 20} x={10} y={TITLE_H + 14} />}
          <Bar fill={i === 0 ? alpha(accent, 0.9) : theme.skeleton} h={8} w={[90, 70, 84, 58, 76][i]} x={22} y={TITLE_H + 25 + i * 36} />
        </g>
      ))}
      <PillButton fill={accent} ink={GHOSTS[win.accent].label} label="+ New" x={w - 74} />
      <text fill={theme.ink} fontFamily={SANS} fontSize={22} fontWeight={700} x={side + 24} y={TITLE_H + 44}>Launch plan</text>
      {[0.86, 0.72, 0.92, 0.55, 0.8, 0.66, 0.74].map((f, i) => (
        <Bar fill={theme.skeleton} key={i} w={(w - side - 48) * f} x={side + 24} y={TITLE_H + 70 + i * 24} />
      ))}
    </g>
  )
}

function PillButton({ fill, ink, label, x }: { fill: string, ink: string, label: string, x: number }) {
  return (
    <g>
      <rect fill={fill} height={22} rx={11} width={58} x={x} y={9} />
      <text fill={ink} fontFamily={SANS} fontSize={12} fontWeight={600} textAnchor="middle" x={x + 29} y={24.5}>{label}</text>
    </g>
  )
}

const REPL_LINES = [
  { input: true, text: 'const notes = await auv.app(\'Notes\')' },
  { input: true, text: 'await notes.findText(\'New\').click()' },
  { input: true, text: 'await notes.type(\'Ship the landing\')' },
  { input: false, text: '✓ 3 steps · run recorded' },
  { input: false, text: '→ captures, AX tree, replay ready' },
]
const INPUT_CHARS = REPL_LINES.filter(l => l.input).reduce((a, l) => a + l.text.length, 0)

function Chart({ theme, win }: { accent: string, theme: Theme, win: WindowNode }) {
  const { h } = win.rect
  const p = win.data.bars ?? 0
  const colors = [GHOSTS.cyan.fill, GHOSTS.pink.fill, GHOSTS.violet.fill, GHOSTS.lime.fill, GHOSTS.amber.fill, GHOSTS.cyan.fill, GHOSTS.violet.fill]
  const heights = [0.45, 0.7, 0.55, 0.9, 0.62, 0.8, 0.5]
  const base = h - 34
  return (
    <g>
      <Bar fill={theme.skeleton} h={10} w={130} x={24} y={TITLE_H + 20} />
      <line stroke={theme.hairline} strokeWidth={1.5} x1={24} x2={win.rect.w - 24} y1={base} y2={base} />
      {heights.map((v, i) => {
        const hh = v * 160 * clamp(p * 1.6 - i * 0.09, 0, 1)
        return <rect fill={alpha(colors[i], 0.8)} height={hh} key={i} rx={7} width={30} x={34 + i * 50} y={base - hh} />
      })}
    </g>
  )
}

/** Scroll scan: a list keeps scrolling while every row that crosses the band is kept. */
function Scan({ accent, theme, win }: { accent: string, theme: Theme, win: WindowNode }) {
  const { h, w } = win.rect
  const scroll = win.data.scroll ?? 0
  const rowH = 44
  const band = TITLE_H + 70
  const first = Math.floor(scroll / rowH)
  const avatars = [GHOSTS.cyan.fill, GHOSTS.pink.fill, GHOSTS.violet.fill, GHOSTS.lime.fill, GHOSTS.amber.fill]
  const kept = first + 2
  return (
    <g>
      <clipPath id={`${win.id}-scan`}><rect height={h - TITLE_H - 34} width={w} x={0} y={TITLE_H} /></clipPath>
      <g clipPath={`url(#${win.id}-scan)`}>
        {Array.from({ length: Math.ceil(h / rowH) + 2 }, (_, k) => {
          const i = first + k
          const y = TITLE_H + 8 + i * rowH - scroll
          const passed = y + rowH / 2 < band + rowH
          return (
            <g key={i} transform={`translate(0 ${y})`}>
              <circle cx={30} cy={rowH / 2} fill={alpha(avatars[i % 5], 0.7)} r={12} />
              <Bar fill={theme.skeleton} h={8} w={(w - 150) * (0.5 + ((i * 37) % 40) / 100)} x={54} y={12} />
              <Bar fill={theme.skeleton} h={7} w={(w - 150) * 0.35} x={54} y={26} />
              {passed && <rect fill={alpha(accent, 0.25)} height={20} rx={10} width={40} x={w - 64} y={12} />}
              {passed && <text fill={accent} fontFamily={SANS} fontSize={11} fontWeight={700} textAnchor="middle" x={w - 44} y={26.5}>kept</text>}
            </g>
          )
        })}
        <rect fill={alpha(accent, 0.1)} height={rowH} rx={10} stroke={accent} strokeDasharray="6 5" strokeWidth={1.5} width={w - 16} x={8} y={band} />
      </g>
      <line stroke={theme.hairline} x1={0} x2={w} y1={h - 34} y2={h - 34} />
      <text fill={theme.ink2} fontFamily={MONO} fontSize={12} x={20} y={h - 12}>{`rows ${kept} · repeats skipped · scanning…`}</text>
    </g>
  )
}

function Term({ accent, win }: { accent: string, theme: Theme, win: WindowNode }) {
  const { w } = win.rect
  let budget = Math.floor((win.data.typed ?? 0) * INPUT_CHARS)
  const output = win.data.output ?? 0
  const t = win.data.t ?? 0
  let caretLine = -1
  let caretX = 0
  const rows = REPL_LINES.map((line, i) => {
    if (!line.input) {
      const shown = clamp(output * 2 - (i - 3), 0, 1)
      return { i, line, opacity: shown, text: line.text }
    }
    const n = clamp(budget, 0, line.text.length)
    budget -= n
    if (n > 0 || caretLine < 0) {
      caretLine = i
      caretX = n
    }
    return { i, line, opacity: n > 0 || i === 0 ? 1 : 0, text: line.text.slice(0, n) }
  })
  return (
    <g>
      <PillButton fill={accent} ink={GHOSTS[win.accent].label} label="▶ Run" x={w - 74} />
      {rows.map(({ i, line, opacity, text }) => (
        <g key={i} opacity={opacity} transform={`translate(22 ${TITLE_H + 38 + i * 30})`}>
          {line.input && <text fill={accent} fontFamily={MONO} fontSize={15}>›</text>}
          <text fill={line.input ? '#e8e6df' : '#86d94a'} fontFamily={MONO} fontSize={15} x={line.input ? 18 : 0}>
            {line.input ? tokens(text).map((tk, k) => <tspan fill={tk.c} key={k}>{tk.s}</tspan>) : text}
          </text>
          {i === caretLine && output === 0 && Math.floor(t * 2.5) % 2 === 0 && (
            <rect fill={accent} height={17} opacity={0.85} width={8} x={18 + caretX * 9.03} y={-13} />
          )}
        </g>
      ))}
    </g>
  )
}

function Todo({ accent, theme, win }: { accent: string, theme: Theme, win: WindowNode }) {
  const { h, w } = win.rect
  const checks = win.data.checks ?? 0
  return (
    <g>
      {[0, 1, 2, 3].map((i) => {
        const cy = 74 + i * 44
        const p = clamp((checks - i) * 1, 0, 1)
        const pop = p > 0 ? easeOutBack(p) : 0
        return (
          <g key={i}>
            <circle cx={30} cy={cy} fill="none" r={10} stroke={theme.ink3} strokeWidth={1.6} />
            <g transform={`translate(30 ${cy}) scale(${pop})`}>
              <circle fill={accent} r={11} />
              <path d="M-5 0 L-1.5 4 L5.5 -4" fill="none" stroke="#fff" strokeLinecap="round" strokeLinejoin="round" strokeWidth={2.4} />
            </g>
            <Bar fill={mix(theme.skeleton, alpha(accent, 0.35), p)} w={(w - 90) * [0.8, 0.62, 0.72, 0.5][i]} x={54} y={cy - 4.5} />
          </g>
        )
      })}
      <text fill={theme.ink2} fontFamily={SANS} fontSize={12} x={24} y={h - 22}>
        {`${Math.min(4, Math.floor(checks + 0.001))} of 4 done`}
      </text>
    </g>
  )
}

function tokens(text: string) {
  return text.split(/('[^']*'|\bawait\b|\bconst\b|auv)/).filter(Boolean).map((s) => {
    if (s.startsWith('\''))
      return { c: '#a6e37d', s }
    if (s === 'await' || s === 'const')
      return { c: '#c4a8ff', s }
    if (s === 'auv')
      return { c: '#6ee7ea', s }
    return { c: '#e8e6df', s }
  })
}

/** Run trace: steps with captures, and a playhead replaying them. */
function Trace({ accent, theme, win }: { accent: string, theme: Theme, win: WindowNode }) {
  const { h, w } = win.rect
  const t = win.data.t ?? 0
  const steps = ['open  Notes', 'findText  “New”', 'click  (412, 88)', 'type  “Ship it”', 'verify  title']
  const head = (t * 0.6) % 1
  const active = Math.floor(head * steps.length)
  return (
    <g>
      {steps.map((s, i) => (
        <g key={s} opacity={i <= active ? 1 : 0.45} transform={`translate(20 ${TITLE_H + 18 + i * 30})`}>
          <circle cx={6} cy={-4} fill={i === active ? accent : i < active ? alpha(accent, 0.55) : theme.ink3} r={5} />
          <text fill={theme.ink} fontFamily={MONO} fontSize={13} x={22} y={0}>{s}</text>
          <text fill={theme.ink3} fontFamily={MONO} fontSize={12} textAnchor="end" x={w - 60} y={0}>{`${12 + i * 7}ms`}</text>
        </g>
      ))}
      {[0, 1, 2, 3, 4].map(i => (
        <rect fill={alpha([GHOSTS.cyan.fill, GHOSTS.pink.fill, GHOSTS.violet.fill, GHOSTS.lime.fill, GHOSTS.amber.fill][i], i <= active ? 0.6 : 0.2)} height={36} key={i} rx={6} width={(w - 40) / 5 - 8} x={20 + i * ((w - 40) / 5)} y={h - 64} />
      ))}
      <rect fill={theme.skeleton} height={4} rx={2} width={w - 40} x={20} y={h - 20} />
      <circle cx={20 + (w - 40) * head} cy={h - 18} fill={accent} r={6} />
    </g>
  )
}
