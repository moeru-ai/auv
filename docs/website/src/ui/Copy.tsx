// Hero copy shared by the desktop and mobile landings, so both say the same.
// NOTICE: copy is provisional.

import { COLORFUL } from '../render/Overlay'

/** One entry per mark part (A, U, V), in the order of `PART_PATHS`. */
export const CORE = [
  { body: 'Clicks and types beside you, without taking your mouse.', title: 'Ghost cursors' },
  { body: 'Drivers that read and operate real app windows.', title: 'App windows' },
  { body: 'Script it line by line. Every run is recorded.', title: 'REPL' },
] as const

/**
 * "Computer Use" with a marker stroke drawn across it, rising a little to the
 * right. The stroke draws once when `play` turns true.
 */
export function Struck({ play }: { play: boolean }) {
  return (
    <span className="struck">
      Computer Use
      {play && (
        <svg aria-hidden="true" preserveAspectRatio="none" viewBox="0 0 100 20">
          <path d="M-3 14 C 18 12.5, 34 11.8, 52 10.2 S 86 7.4, 103 6" stroke={COLORFUL[2]} />
        </svg>
      )}
    </span>
  )
}
