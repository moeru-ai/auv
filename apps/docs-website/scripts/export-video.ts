// Export the intro film to MP4.
//
// Glass is CSS backdrop-filter, which only a browser compositor can draw, so we
// run the /render route in headless Chrome, set each frame's time explicitly
// (never wall-clock playback, so no dropped frames), screenshot it, and pipe
// the PNGs into ffmpeg.
//
//   pnpm video [--theme light|dark] [--layout landscape|portrait] [--fps 60] [--scale 1|2] [--out file.mp4] [--from 0] [--to 16]

import process from 'node:process'

import { Buffer } from 'node:buffer'
import { spawn } from 'node:child_process'
import { once } from 'node:events'
import { mkdir } from 'node:fs/promises'
import { dirname } from 'node:path'
import { parseArgs } from 'node:util'

import { chromium } from 'playwright'
import { createServer } from 'vite'

const { values } = parseArgs({
  options: {
    fps: { default: '60', type: 'string' },
    from: { type: 'string' },
    layout: { default: 'landscape', type: 'string' },
    out: { type: 'string' },
    scale: { default: '1', type: 'string' },
    theme: { default: 'light', type: 'string' },
    to: { type: 'string' },
  },
})

const fps = Number(values.fps)
const scale = Number(values.scale)
const portrait = values.layout === 'portrait'
const W = portrait ? 1080 : 1920
const H = portrait ? 1920 : 1080
const out = values.out ?? `out/auv-intro-${values.theme}-${W * scale}x${H * scale}-${fps}fps.mp4`

await mkdir(dirname(out), { recursive: true })

const server = await createServer({ logLevel: 'warn', server: { host: '127.0.0.1', port: 0 } })
await server.listen()
const base = server.resolvedUrls!.local[0]

// NOTICE: GPU compositing makes backdrop-filter frames several times faster
// than the default software path in headless mode.
const browser = await chromium.launch({ args: ['--enable-gpu', '--use-angle=default', '--ignore-gpu-blocklist'] })
try {
  const page = await browser.newPage({ deviceScaleFactor: scale, viewport: { height: H, width: W } })
  const cdp = await page.context().newCDPSession(page)
  // NOTICE: with a raw CDP session attached, Playwright's deviceScaleFactor did not
  // reach the page (devicePixelRatio stayed 1, so --scale 2 exported 1080p).
  // Set the metrics on the session itself so WebGL sizes its buffer for the scale.
  await cdp.send('Emulation.setDeviceMetricsOverride', { deviceScaleFactor: scale, height: H, mobile: false, width: W })
  await page.goto(`${base}render?theme=${values.theme}&layout=${values.layout}`)
  await page.waitForFunction(() => typeof (window as any).__auvSetT === 'function')
  await page.evaluate(() => document.fonts.ready)
  const duration: number = await page.evaluate(() => (window as any).__auvDuration)
  const dpr: number = await page.evaluate(() => devicePixelRatio)
  if (dpr !== scale)
    throw new Error(`page devicePixelRatio is ${dpr}, expected ${scale}`)
  const from = Number(values.from ?? 0)
  const to = Number(values.to ?? duration)
  const total = Math.round((to - from) * fps)

  const ffmpeg = spawn('ffmpeg', [
    '-y',
    '-loglevel',
    'error',
    '-f',
    'image2pipe',
    '-framerate',
    String(fps),
    '-i',
    '-',
    '-c:v',
    'libx264',
    '-preset',
    'slow',
    '-crf',
    '14',
    '-pix_fmt',
    'yuv420p',
    '-movflags',
    '+faststart',
    out,
  ], { stdio: ['pipe', 'inherit', 'inherit'] })

  const started = Date.now()
  for (let i = 0; i <= total; i++) {
    const t = from + i / fps
    await page.evaluate(t => (window as any).__auvSetT(t), t)
    // Raw CDP capture: `optimizeForSpeed` skips the slow zlib level Playwright uses.
    const { data } = await cdp.send('Page.captureScreenshot', { clip: { height: H, scale: 1, width: W, x: 0, y: 0 }, format: 'png', optimizeForSpeed: true })
    const png = Buffer.from(data, 'base64')
    if (!ffmpeg.stdin.write(png))
      await once(ffmpeg.stdin, 'drain')
    if (i % 15 === 0 || i === total) {
      const rate = (i + 1) / ((Date.now() - started) / 1000)
      process.stderr.write(`\rframe ${i}/${total}  ${rate.toFixed(1)} fps`)
    }
  }
  ffmpeg.stdin.end()
  const [code] = await once(ffmpeg, 'close')
  process.stderr.write(`\n${code === 0 ? `wrote ${out}` : `ffmpeg exited with ${code}`}\n`)
  process.exitCode = code ?? 1
}
finally {
  await browser.close()
  await server.close()
}
