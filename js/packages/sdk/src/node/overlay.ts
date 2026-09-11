import type { Color, Shadow } from '../gen/auv/api/driver/v1/overlay_pb'

/** Overlay presentation configured when launching an app-owned daemon. */
export interface AuvOverlayOptions {
  /**
   * Host theme inherited by the daemon's Runner children. Replaces any
   * AUV_OVERLAY_THEME from the inherited or explicitly supplied environment.
   * An empty object clears those overrides. Restart the daemon to change it.
   * @default undefined Uses AUV_OVERLAY_THEME from the launch environment.
   */
  theme?: OverlayTheme
}

/** Resolved hex color (#RRGGBB or #RRGGBBAA) or normalized RGBA channels in [0, 1]. */
export type OverlayColor = Omit<Color, '$typeName'> | string

/** Built-in cursor selection or SVG artwork; SVG requires a supported native adapter. */
export type OverlayCursorImage = {
  /** Render custom vector artwork. */
  kind: 'svg'
  /** Non-empty SVG source, at most 256 KiB; passed through without modifying its contents. */
  source: string
} | {
  /** Select platform-provided artwork. */
  kind: 'builtIn'
  /** AUV, its click state, or the user cursor. */
  variant: 'auv' | 'auvClick' | 'you'
}

/** Native cursor silhouette shadow. Dimensions are logical display points. */
export interface OverlayCursorShadow extends Omit<Shadow, '$typeName' | 'color'> {
  /** Normalized RGBA channels. Set alpha to zero to suppress the built-in glow. */
  color: Omit<Color, '$typeName'>
}

/** Partial host appearance overrides. Unset fields preserve the layer's existing style. */
export interface OverlayTheme {
  /** Replacement cursor artwork. Built-in artwork has a fixed palette. */
  cursorImage?: OverlayCursorImage
  /** Label background; also colors the Windows built-in sprite. */
  cursorLabelBackground?: OverlayColor
  /** Cursor label text color. */
  cursorLabelForeground?: OverlayColor
  /** Native silhouette shadow, excluding the label; requires a supported native adapter. */
  cursorShadow?: OverlayCursorShadow
  /** Border color for outline layers, including capture frames. */
  outlineColor?: OverlayColor
  /** Status pill background, including opacity. */
  statusBackground?: OverlayColor
  /** Status pill text color. */
  statusForeground?: OverlayColor
}
