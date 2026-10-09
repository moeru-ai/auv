import { FILM } from '../scene/film'

/** The film is exported by driving /render in headless Chrome; this page documents it. */
export function ExportPage() {
  return (
    <div style={{ lineHeight: 1.6, margin: '0 auto', maxWidth: 760, padding: 32 }}>
      <h1 style={{ fontSize: 24 }}>Export intro film</h1>
      <p>
        The desk is drawn with WebGL and the mark with SVG on top, so export runs the
        {' '}
        <a href="/render">/render</a>
        {' '}
        route in headless Chrome, steps through every frame of the
        {` ${FILM.duration}s `}
        film, screenshots each one, and pipes them into ffmpeg.
      </p>
      <pre style={{ background: 'var(--panel)', border: '1px solid var(--edge)', borderRadius: 12, overflowX: 'auto', padding: 16 }}>
        {`pnpm video:landscape:light                # light, 1920x1080, 60 fps
pnpm video:landscape:dark --scale 2       # dark, 3840x2160
pnpm video:portrait:light                 # light, 1080x1920
pnpm video:all                            # all four presets, into out/
pnpm video --fps 30 --out out/intro.mp4   # no preset`}
      </pre>
      <p>
        Preview a single frame:
        {' '}
        <a href="/render?t=2.2&theme=dark">/render?t=2.2&amp;theme=dark</a>
        {' · '}
        <a href="/film">open the player</a>
      </p>
    </div>
  )
}
