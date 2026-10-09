// Paging over the native window scroll, for the mobile landing's scrubbed
// story (hero -> gallery windows -> install section).
//
// CSS scroll snap picks the snap nearest the projected momentum endpoint, so a
// slow swipe sprang back to where it started and a long fling skipped windows.
// Inside the story range this hook owns the gesture instead:
//
//   touch   the page follows the finger 1:1, so holding and dragging slowly
//           scrubs the story. On release, past a quarter of the way to the
//           next snap the page carries on to it; anything less springs back.
//   wheel   the same, with the wheel or trackpad as the finger: the page
//           follows the deltas (within one snap either way), and the gesture
//           "releases" once the wheel goes quiet.
//   other   keyboard, scrollbar, scrollIntoView, or native momentum coming up
//           out of the install section: when scrolling stops between snaps,
//           settle on the next snap in the direction of travel.
//
// Past the last snap (the install section) scrolling is native again; from the
// last snap, only a pull back toward the story is taken over.

import { clamp } from 'es-toolkit'
import { useEffect, useRef } from 'react'

/** Share of the way to the next snap past which a release carries on to it. */
const COMMIT_SHARE = 0.25
/**
 * Settle spring, slightly underdamped: it keeps the release velocity and eases
 * in with a soft give instead of stopping dead (about 0.6 s).
 */
const STIFFNESS = 120
const DAMPING = 18
/** Quiet time that ends a wheel gesture or a native scroll. */
const WHEEL_QUIET_MS = 140
const SCROLL_IDLE_MS = 140

/** Pages `window` scroll between ascending `snaps` (scrollY values) while `enabled`. */
export function useSnapPaging(snaps: number[], enabled: boolean) {
  const snapsRef = useRef(snaps)
  snapsRef.current = snaps

  useEffect(() => {
    if (!enabled)
      return
    const last = () => snapsRef.current[snapsRef.current.length - 1]
    const nearest = (y: number) => {
      const s = snapsRef.current
      let best = 0
      for (let i = 1; i < s.length; i++) {
        if (Math.abs(s[i] - y) < Math.abs(s[best] - y))
          best = i
      }
      return best
    }

    // --- Settle: a spring toward one snap, seeded with the release velocity.
    let raf = 0
    let gliding = false
    let glideTo = 0
    const stop = () => {
      cancelAnimationFrame(raf)
      gliding = false
    }
    const glide = (i: number, v0: number, start = scrollY) => {
      stop()
      const s = snapsRef.current
      glideTo = clamp(i, 0, s.length - 1)
      const target = s[glideTo]
      let pos = start
      let vel = v0
      let prev = performance.now()
      gliding = true
      const step = (now: number) => {
        // NOTICE: fixed 4 ms substeps keep the explicit integration stable and
        // the settle on wall-clock time even at a few fps (software-GL emulators).
        let left = Math.min(0.25, (now - prev) / 1000)
        prev = now
        while (left > 0) {
          const dt = Math.min(0.004, left)
          left -= dt
          vel += (STIFFNESS * (target - pos) - DAMPING * vel) * dt
          pos += vel * dt
        }
        if (Math.abs(target - pos) < 0.5 && Math.abs(vel) < 20) {
          gliding = false
          scrollTo(0, target)
          return
        }
        scrollTo(0, pos)
        raf = requestAnimationFrame(step)
      }
      raf = requestAnimationFrame(step)
    }
    /**
     * Where to settle after moving in direction `dir`: the snap passed last,
     * or the next one once `y` is past `COMMIT_SHARE` of the way to it
     * (`commit` skips that check).
     */
    const settleIndex = (y: number, dir: number, commit = false) => {
      const s = snapsRef.current
      if (dir === 0)
        return nearest(y)
      let behind = dir > 0 ? 0 : s.length - 1
      for (let i = 0; i < s.length; i++) {
        if (dir > 0 && s[i] <= y + 0.5)
          behind = i
        if (dir < 0 && s[s.length - 1 - i] >= y - 0.5)
          behind = s.length - 1 - i
      }
      const ahead = clamp(behind + dir, 0, s.length - 1)
      return commit || Math.abs(y - s[behind]) > Math.abs(s[ahead] - s[behind]) * COMMIT_SHARE ? ahead : behind
    }
    /**
     * Release at `y` (the last position the gesture set): settle relative to
     * the snap the gesture started from, keeping its speed.
     *
     * NOTICE: `y` is passed in rather than read from `scrollY`. With WebKit's
     * async scrolling a stale UI-side position can land between the last
     * touchmove and touchend, so on iOS a fast drag read back an older,
     * shorter position and sprang back.
     */
    const release = (from: number, v: number, y: number) => {
      // The seed is capped so a hard fling gives a little, not a visible
      // overshoot into the neighbouring window.
      glide(settleIndex(y, Math.sign(y - snapsRef.current[from])), clamp(v, -1200, 1200), y)
    }

    // --- Touch: follow the finger, settle on release.
    interface Drag { finger: number, from: number, id: number, own: boolean | null, samples: { t: number, y: number }[], start: number }
    let drag: Drag | null = null
    const onTouchStart = (e: TouchEvent) => {
      if (e.touches.length !== 1 || scrollY > last() + 1) {
        drag = null
        return
      }
      const t = e.touches[0]
      // Catching a settling page holds it where it is, measured from its target.
      const from = gliding ? glideTo : nearest(scrollY)
      stop()
      drag = { finger: t.clientY, from, id: t.identifier, own: null, samples: [{ t: e.timeStamp, y: scrollY }], start: scrollY }
    }
    const onTouchMove = (e: TouchEvent) => {
      const t = drag && [...e.changedTouches].find(c => c.identifier === drag!.id)
      if (!drag || !t)
        return
      const dy = t.clientY - drag.finger
      // NOTICE: iOS commits to native scrolling on the first moves, so the
      // decision is made on the first touchmove, before any threshold.
      if (drag.own === null)
        drag.own = drag.start < last() - 1 || dy > 0
      if (!drag.own)
        return
      e.preventDefault()
      const y = clamp(drag.start - dy, 0, last())
      scrollTo(0, y)
      drag.samples.push({ t: e.timeStamp, y })
      while (drag.samples.length > 2 && e.timeStamp - drag.samples[0].t > 100)
        drag.samples.shift()
    }
    const onTouchEnd = (e: TouchEvent) => {
      const d = drag
      if (!d || ![...e.changedTouches].some(c => c.identifier === d.id))
        return
      drag = null
      if (!d.own) {
        // A tap that caught a settling page: let it finish.
        if (d.own === null && scrollY < last() - 0.5 && !snapsRef.current.some(p => Math.abs(p - scrollY) < 1))
          glide(d.from, 0)
        return
      }
      const a = d.samples[0]
      const b = d.samples[d.samples.length - 1]
      // The release speed only seeds the spring, so the hand-off is smooth; a
      // finger that stopped before lifting carries none.
      const v = b.t > a.t && e.timeStamp - b.t < 80 ? ((b.y - a.y) / (b.t - a.t)) * 1000 : 0
      release(d.from, v, b.y)
    }

    // --- Wheel: the same, with wheel deltas as the finger.
    let wheel: null | { from: number, y: number } = null
    let wheelTimer = 0
    let nativeWheelAt = -Infinity
    const onWheel = (e: WheelEvent) => {
      if (e.ctrlKey)
        return
      const dy = e.deltaY * (e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? innerHeight : 1)
      if (Math.abs(e.deltaX) > Math.abs(dy))
        return
      const y = scrollY
      // Inside the story, or pulling back up from the install section's top.
      const ours = y < last() - 1 || (y <= last() + 1 && dy < 0)
      if (!ours) {
        nativeWheelAt = e.timeStamp
        return
      }
      e.preventDefault()
      clearTimeout(wheelTimer)
      // Inertia carried up out of the install section stops at its top.
      const carried = e.timeStamp - nativeWheelAt < WHEEL_QUIET_MS
      if (carried)
        nativeWheelAt = e.timeStamp
      if (!wheel && !carried) {
        wheel = { from: gliding ? glideTo : nearest(y), y }
        stop()
      }
      if (wheel) {
        // Follow, but within one snap either way, so trackpad inertia cannot
        // run through several windows.
        const s = snapsRef.current
        const lo = s[Math.max(0, wheel.from - 1)]
        const hi = s[Math.min(s.length - 1, wheel.from + 1)]
        wheel.y = clamp(wheel.y + dy, lo, hi)
        scrollTo(0, wheel.y)
      }
      wheelTimer = window.setTimeout(() => {
        const w = wheel
        wheel = null
        if (w)
          release(w.from, 0, w.y)
      }, WHEEL_QUIET_MS)
    }

    // --- Anything else: settle once scrolling goes idle between snaps.
    let lastY = scrollY
    let lastDir = 0
    let idleTimer = 0
    const onScroll = () => {
      const y = scrollY
      if (y !== lastY)
        lastDir = Math.sign(y - lastY)
      lastY = y
      if (drag?.own || wheel || gliding)
        return
      clearTimeout(idleTimer)
      idleTimer = window.setTimeout(() => {
        const y = scrollY
        if (drag || wheel || gliding || y >= last() - 0.5 || snapsRef.current.some(p => Math.abs(p - y) < 1))
          return
        glide(settleIndex(y, lastDir, true), 0)
      }, SCROLL_IDLE_MS)
    }

    const active = { passive: false }
    addEventListener('touchstart', onTouchStart, { passive: true })
    addEventListener('touchmove', onTouchMove, active)
    addEventListener('touchend', onTouchEnd)
    addEventListener('touchcancel', onTouchEnd)
    addEventListener('wheel', onWheel, active)
    addEventListener('scroll', onScroll, { passive: true })
    return () => {
      stop()
      clearTimeout(wheelTimer)
      clearTimeout(idleTimer)
      removeEventListener('touchstart', onTouchStart)
      removeEventListener('touchmove', onTouchMove)
      removeEventListener('touchend', onTouchEnd)
      removeEventListener('touchcancel', onTouchEnd)
      removeEventListener('wheel', onWheel)
      removeEventListener('scroll', onScroll)
    }
  }, [enabled])
}
