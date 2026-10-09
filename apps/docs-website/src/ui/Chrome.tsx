// Page chrome shared by the desktop and mobile landings: a nav bar (brand mark
// left, GitHub + theme icons right) and a floating island for playback controls
// (dev only).

import type { ReactNode } from 'react'

import type { IconName } from '../lib/icons'
import type { Theme } from '../theme'

import { ICON_CENTER, PART_PATHS } from '../lib/icon'
import { ICONS } from '../lib/icons'

export function Icon({ name, size = 20 }: { name: IconName, size?: number }) {
  const i = ICONS[name]
  return <svg aria-hidden="true" dangerouslySetInnerHTML={{ __html: i.body }} height={size} viewBox={`0 0 ${i.w} ${i.h}`} width={size} />
}

/** Brand mark height in the nav bar, CSS px. */
export const NAV_MARK_H = 22
export const NAV_H = 56

/** Playback controls for reviewing the landing; pages render it in dev builds only. */
export function Island({ bottom = 0, children }: { bottom?: number, children: ReactNode }) {
  return <div className="island" style={{ bottom: bottom + 18 }}>{children}</div>
}

export function IslandButton({ href, icon, label, onClick }: { href?: string, icon: IconName, label: string, onClick?: () => void }) {
  const body = (
    <>
      <Icon name={icon} size={17} />
      <span>{label}</span>
    </>
  )
  return href
    ? <a className="island-btn" href={href}>{body}</a>
    : <button className="island-btn" onClick={onClick}>{body}</button>
}

export function NavBar({ brand = 1, onToggleTheme, safeTop = 0, theme }: { brand?: number, onToggleTheme: () => void, safeTop?: number, theme: Theme }) {
  return (
    <nav className="nav" style={{ height: NAV_H + safeTop, paddingTop: safeTop }}>
      <a aria-label="AUV" className="nav-brand" href="/?skip" style={{ opacity: brand, pointerEvents: brand > 0.5 ? 'auto' : 'none' }}>
        <NavMark theme={theme} />
      </a>
      <div className="nav-actions">
        <a aria-label="GitHub" className="nav-icon" href="https://github.com/moeru-ai/auv"><Icon name="github-logo" /></a>
        <button aria-label="Toggle theme" className="nav-icon" onClick={onToggleTheme}>
          <Icon name={theme.name === 'light' ? 'moon' : 'sun'} />
        </button>
      </div>
    </nav>
  )
}

export function NavMark({ height = NAV_MARK_H, theme }: { height?: number, theme: Theme }) {
  const s = height / 92.4
  return (
    <svg aria-hidden="true" height={height} viewBox={`${ICON_CENTER.x - 110.5} 96 221 92.4`} width={221 * s}>
      {PART_PATHS.map((d, i) => <path d={d} fill={theme.icon[i]} key={i} />)}
    </svg>
  )
}

/** Where the nav brand mark sits (center), for elements that fly into it. */
export function navMarkCenter(safeTop: number) {
  return { x: 18 + (221 * NAV_MARK_H) / 92.4 / 2, y: safeTop + NAV_H / 2 }
}
