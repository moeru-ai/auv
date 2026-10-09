import type { Pt } from '../lib/shape'
import type { Slab } from '../lib/slab'
import type { GhostColor } from '../theme'

/**
 * Soft radial light. `color` is a theme base-palette index or a CSS color.
 * Window glows carry their window's layer so the 3D view puts them behind it.
 */
export interface Blob { alpha: number, color: number | string, depth?: number, r: number, x: number, y: number, z?: number }

/**
 * Camera for the opening, around the desk's display. The front view is
 * yaw = pitch = 0, zoom 1, looking at the origin; no camera means exactly that
 * view. `persp` blends orthographic (0) into a standard perspective (1); the
 * look-at plane frames the same either way, so only depth changes.
 */
export interface Camera { at: Vec3, persp: number, pitch: number, yaw: number, zoom: number }

export interface CursorNode {
  color: GhostColor
  id: string
  kind: 'ghost' | 'user'
  label?: LabelState
  opacity: number
  /** Set while the cursor morphs into an icon part. */
  outline?: string
  outlineFill?: string
  scale: number
  x: number
  y: number
}

/** A faint card standing for one display or device in the opening's line-up. */
export interface DisplayNode {
  /** Card center, world space. Cards are parallel to the desk plane. */
  center: Vec3
  h: number
  id: string
  opacity: number
  radius: number
  /** Defocus: edge feather in world px (0 = sharp). */
  soft: number
  w: number
}

export interface IconNode {
  /** 0..1 blend from grayscale toward the colorful palette; one value or per part (A, U, V). */
  color: [number, number, number] | number
  cx: number
  cy: number
  opacity: number
  s: number
}

export interface LabelState {
  /** Number of characters revealed. */
  chars: number
  /** 0..1 pop-in scale progress (may overshoot). */
  pop: number
  text: string
}

export interface Rect { h: number, w: number, x: number, y: number }

export interface Ripple { color: string, p: number, x: number, y: number }

export interface SceneState {
  /** Faint base field, screen space. */
  blobs: Blob[]
  /** Opening camera; without one the desk is drawn flat, front on. */
  camera?: Camera
  cursors: CursorNode[]
  /**
   * The opening's display line-up. The desk lives in the display at the world
   * origin: cards behind it draw before the desk, cards in front after it.
   */
  displays?: DisplayNode[]
  /** Per-window glows, desk-plane space (under the camera). */
  glows?: Blob[]
  icon?: IconNode
  ripples: Ripple[]
  windows: WindowNode[]
}

/** World point for the opening: stage px, x right, y up, z toward the viewer, origin at the stage center. */
export type Vec3 = [number, number, number]

export interface WindowData {
  bars?: number
  /** Fractional count of checked rows; the fraction drives the check pop. */
  checks?: number
  /** 0..1 of the REPL output revealed. */
  output?: number
  /** Seconds since playback started, negative when paused. */
  playing?: number
  scroll?: number
  /** Scene time, for idle loops such as the caret blink. */
  t?: number
  /** 0..1 of the REPL input typed so far. */
  typed?: number
}

export type WindowKind = 'browser' | 'chart' | 'music' | 'notes' | 'scan' | 'term' | 'todo' | 'trace'

export interface WindowNode {
  accent: GhostColor
  /** 0..1 visibility of chrome and content. */
  chrome: number
  /**
   * Size the content texture is drawn at, when it differs from `rect` (blocks
   * stretching open). Content is rasterized once at this size and scaled.
   */
  contentSize?: { h: number, w: number }
  data: WindowData
  /** Extra world-space depth offset (the mark before it docks into its display). */
  depth?: number
  /** Tint override (morph color). */
  fill?: string
  /**
   * Skip backdrop-filter and draw a plain translucent fill. For windows that
   * sit under another blur anyway, where their own glass would be wasted GPU work.
   */
  flat?: boolean
  /** Multiplier for this window's glow (0 hides it). */
  glow?: number
  id: string
  kind: WindowKind
  opacity: number
  origin?: { x: number, y: number }
  /** Arbitrary outline in stage coordinates (logo part -> block morph). */
  poly?: Pt[]
  /** Corner radius override for the rect body (default 18). */
  radius?: number
  rect: Rect
  /** Uniform scale about `origin` (window-local), for spring pops and exits. */
  scale?: number
  /** Set while the window morphs; drawn as this clipped shape instead of a rect. */
  slab?: Slab
  title: string
  z: number
}
