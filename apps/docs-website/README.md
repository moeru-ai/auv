# AUV website

The AUV website (`@auv-js/docs-website`): an intro film in which app windows and
ghost cursors fold into the AUV mark, an interactive landing with the hero mark
and an install guide, and a video exporter for the film.

> Status: exploratory prototype. Copy, timing, and names are provisional.

```sh
pnpm install                              # from the repository root
pnpm -F @auv-js/docs-website dev               # http://127.0.0.1:5190
pnpm -F @auv-js/docs-website build             # static site in dist/
pnpm -F @auv-js/docs-website typecheck
pnpm lint                                  # root ESLint + oxlint config
```

The commands below run from this directory (`apps/docs-website/`).

| Route     | What it is |
| --------- | ---------- |
| `/`       | Landing. Wide screens: the film, then the interactive mark over a frosted ambient desk (hover or tap a desk window to reveal it). Portrait / narrow screens: the 9:16 film; then a smaller gray mark rests in the lower third over the frosted desk, with the tagline "Application Use Via / programmable ~~Computer Use~~". Colored copies of its parts fly up and unfold like the desktop hero (A → ghost cursor, U → app windows, V → line pointer + REPL) with a capability subtitle; they cycle on their own and a tap on a part switches. Scrolling sends the mark into the nav bar and lines the background windows up into a row, each with its own ghost cursor (the REPL has none: it runs code) and a box-less title and description in the lower start corner (right in RTL), scrubbed by scroll with a light snap per window. Both layouts use the same hero copy (`src/ui/Copy.tsx`), share a nav bar (mark, GitHub, theme) and a floating island (replay, install), and scroll on to an Install section after the intro (desktop: the gray mark flies up and docks above it): system tabs (macOS / Linux / Windows), install-method tabs (Base UI), a CPU toggle where the download differs, copyable command blocks, and what the helper is for. Now and then the mark's A or V turns into a cyan or pink ghost cursor and clicks an install-method tab. Hero text swaps (tagline <-> a part's capability) use the transitions.dev "Texts reveal". `?skip` jumps past the intro, `?theme=light\|dark` forces a theme, `?layout=mobile\|desktop` forces a layout. |
| `/film`   | Film player with scrubber, loop, theme, and 16:9 / 9:16 toggle. `?t=7.5` opens paused on a frame, `?layout=portrait` starts in 9:16. |
| `/render` | Bare frame target for the exporter (1920x1080, or 1080x1920 with `?layout=portrait`). `?t=2.2&theme=dark` shows one frame; `window.__auvSetT(t)` repaints. |
| `/export` | How to export. |
| `/card`   | Static cards over the frosted ambient desk. `?kind=og` (1200x630) and `social` (1280x640, GitHub social preview): "Application Use Via ..." top left, the mark and headline on the left, one crayon stroke curling from the mark into a loose column of dashed square tiles (agent apps) on the right. `?kind=readme` (1280x480, rounded): the mark with the headline beside it. `?theme=light\|dark`. |

## Cards

```sh
pnpm cards   # public/og-*.png, public/social-preview-*.png, ../../docs/assets/readme-banner-*.png
```

`index.html` points `og:image` and `twitter:image` at `/og-light.png`. Upload
`public/social-preview-*.png` by hand in the GitHub repository settings
(Settings -> General -> Social preview); GitHub has no API for it. Re-run
`pnpm cards` after changing the mark, the tagline, or the ambient desk.

## Deploy

`auv.moeru.ai` is one assets-only Cloudflare Worker (`wrangler.toml`).
docs-website is the site root, and repl-playground is merged into it under
`/playground/`:

```sh
pnpm build:site   # dist/ = docs-website, dist/playground/ = apps/repl-playground/dist
```

The `Cloudflare Workers` workflow runs `build:site` and deploys on every push to
`main`. The preview workflows upload the same build for each pull request.
Routes are managed in the Cloudflare dashboard.

## Export

The film exporter is a package script. Videos are written to `out/`, which is
ignored by Git.

| Script | Output |
| ------ | ------ |
| `pnpm video:landscape:light` | 16:9, light theme |
| `pnpm video:landscape:dark` | 16:9, dark theme |
| `pnpm video:portrait:light` | 9:16, light theme |
| `pnpm video:portrait:dark` | 9:16, dark theme |
| `pnpm video:all` | all four, one after another |
| `pnpm video` | no preset; set every flag yourself |

Each script accepts more flags after it:

```sh
pnpm video:landscape:dark --scale 2              # 3840x2160
pnpm video:portrait:light --scale 2 --fps 120    # 2160x3840 at 120 fps
pnpm video --fps 30 --from 0 --to 4 --out out/clip.mp4
```

| Flag | Default | Meaning |
| ---- | ------- | ------- |
| `--theme` | `light` | `light` or `dark` |
| `--layout` | `landscape` | `landscape` (1920x1080) or `portrait` (1080x1920) |
| `--scale` | `1` | device scale; `0.5`, `1`, `2` |
| `--fps` | `60` | frames per second |
| `--from`, `--to` | whole film | time range in seconds |
| `--out` | `out/auv-intro-<theme>-<w>x<h>-<fps>fps.mp4` | output file |

The exporter needs `ffmpeg` on `PATH` and the Playwright Chromium browser
(`pnpm exec playwright install chromium`).

The 9:16 film is the same choreography: windows get portrait rects, and every
cursor key, click, and spawn point is replayed as an offset inside its window
(`FilmLayout` / `mapPoint` in `src/scene/film.ts`).

The script starts Vite, opens `/render` in headless Chrome (Playwright, GPU
flags on), sets each frame's time explicitly, screenshots it over CDP at the
requested device scale, and pipes PNGs to `ffmpeg` (libx264, CRF 14). Needs
`ffmpeg` on PATH. About 17 frames/s at 1080p and 11 at 4K on Apple Silicon.

## How the mark is built

The mark is three shapes from the macOS helper icon
([`crates/auv-device-helper-macos/package/AUV Helper.icon`](../../crates/auv-device-helper-macos/package/AUV%20Helper.icon)):

| Part | Shape | Light | Dark | Becomes |
| ---- | ----- | ----- | ---- | ------- |
| A | upward rounded triangle | `#242424` | `#FFFFFF` | a ghost cursor |
| U | sheared rounded slab | `#4A4A4A` | `#AEAEAE` | app windows |
| V | downward rounded triangle | `#797979` | `#5E5E5E` | the REPL |

## Storyboard (18.2 s)

| Time | Beat |
| ---- | ---- |
| 0.0 – 0.45 | The gray mark holds (same frame as the end, so the film loops). |
| 0.45 – 1.85 | The camera turns out to a high orthographic 3/4 view over a line-up of faint display cards (half desk size, one per desktop or device) drifting from bottom right to top left; far cards are defocused. A, U, V glide up the line onto three separate displays and change shape only as they settle: A and V into ghost cursors, the U (the ending's springs, backwards) into a plain window block. All three fade to a docked gray. |
| 1.75 – 5.0 | The line-up runs: it speeds up, streams dozens of displays past with motion trails (the docked ones fly off with it), then brakes so a fresh display slides in from behind and stops. The camera turns into a standard perspective and yaws toward the line, landing on the front view while the line is still slightly oblique. That display lights up (light theme: sharp glass with a shadow) and opens to full size, the cards in front of it fade left to right, and every window blooms in it. |
| 5.2 – 6.4 | The user's pointer arrives and clicks into the browser. |
| 5.95 – 9.35 | A cyan ghost cursor springs out beside it with a label, drags Notes aside, clicks "New". |
| 9.15 – 10.35 | A Checklist window springs out of that click; pink and violet ghosts join. |
| 10.35 – 14.15 | Everyone works at once: checking tasks, typing a REPL script, moving windows, playing music; a REPL run springs out a chart and a lime ghost. |
| 13.85 – 14.85 | The last two ghost clicks; the user's pointer fades out. |
| 14.15 – 15.95 | Other windows leave. The browser shrinks, leans into the shear, then pulls its bottom edge round into the "U"; the cyan ghost becomes the "A", the pink ghost the "V". |
| 16.4 – 18.2 | A quick eased color change from ghost colors to the mark's grays, then hold. |

The opening camera blends from orthographic into a standard perspective; the
look-at plane frames the same at any blend. It looks at the display it settles
on, so its end pose (front on, zoom 1) matches the flat desk view and it is
dropped at 5.0 s with no seam. The run is a speed curve (ramp, hold, cubic
brake) whose peak is solved so the stream ends exactly on that display. The "U" opening runs `windowToU` backwards.

The window -> "U" transition is parametric, not a point morph: the "U" is a
68.01 x 92.42 rounded rect with quarter-ellipse corners, sheared by 0.3839
(see `src/lib/slab.ts`). Three springs drive shrink, lean, and the bottom
round in turn, and the last frame matches the icon path.

## Structure

```text
apps/docs-website/
├── index.html
├── public/                  rendered Open Graph and social preview cards (pnpm cards)
├── scripts/export-video.ts   headless Chrome + ffmpeg film exporter (pnpm video:*)
├── scripts/export-cards.ts   headless Chrome card exporter (pnpm cards)
└── src/
    ├── main.tsx              routes: /, /film, /render, /export
    ├── theme.ts              light / dark tokens, ghost colors
    ├── styles.css
    ├── pages/                one component per route (desktop and mobile landing)
    ├── scene/                the film and the ambient desk as pure functions of time
    ├── gl/                   three.js renderer for scene states
    ├── render/               DOM / SVG drawing of scene parts (windows, cursors, glass)
    ├── hero/                 the interactive mark (desktop and mobile)
    ├── install/              install section and its per-system methods
    ├── ui/                   page chrome: nav bar, island, copy, text reveal, fit-to-viewport
    └── lib/                  math, shapes, springs, icons, small hooks
```

- `src/scene/film.ts` — the film as a pure function `film(t, theme) -> SceneState`.
- `src/scene/ambient.ts` — looping background desk for the landing, with per-window explainers.
- `src/gl/DeskGL.ts` — three.js renderer: flat front view, or the opening camera (orthographic blending into perspective) over the display line-up with defocused, trailing cards, per-window glass (blur of everything drawn behind it, 1/8-res pyramid + gaussian), glows, shadows, cursors, and the landing's frosted veil with its hover hole.
- `src/gl/svgcanvas.ts` — draws the window content JSX onto Canvas 2D synchronously, so content textures are deterministic for export.
- `src/render/Overlay.tsx` — crisp SVG layer for cursor -> triangle morphs and the final mark.
- `src/hero/HeroIcon.tsx` — hover-to-unfold mark (A: ghost cursor, U: windows, V: line pointer + REPL), DOM with CSS glass over the canvas.
- `src/hero/MobileHero.tsx` — mobile counterpart of the desktop hero unfolds, flying out of the gray mark.
- `src/pages/MobileLanding.tsx` — portrait landing: 9:16 film, capability showcase grown from the mark, scroll-driven gallery.
- `src/install/` — install section and its per-system methods, mirrored from the AUV README's Getting Started.
- `src/ui/Chrome.tsx` — nav bar and floating island (Phosphor icons from `src/lib/icons.ts`).
- `src/lib/slab.ts` — parametric sheared slab and the window -> "U" stages.
- `src/lib/spring.ts` — closed-form spring, so film frames stay deterministic.
