// Export the static cards from the /card route to PNG.
//
// Like the film exporter, the frosted desk is WebGL plus CSS, so we render the
// route in headless Chrome and screenshot the card element.
//
//   pnpm cards
//
// Outputs (light and dark each):
//   public/og-{theme}.png                     1200x630  Open Graph image (index.html)
//   public/social-preview-{theme}.png         1280x640  GitHub repository social preview
//   ../../docs/assets/readme-banner-{theme}.png  2560x960 README banner, rounded and transparent at the corners

import type { CardName } from '../src/pages/Card'

import { mkdir } from 'node:fs/promises'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

import { chromium } from 'playwright'
import { createServer } from 'vite'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')

const JOBS: { kind: CardName, out: (theme: string) => string, scale: number }[] = [
  { kind: 'og', out: theme => join(root, 'public', `og-${theme}.png`), scale: 1 },
  { kind: 'social', out: theme => join(root, 'public', `social-preview-${theme}.png`), scale: 1 },
  // Retina-sharp at GitHub's README width; the <img> scales it down.
  { kind: 'readme', out: theme => join(root, '..', '..', 'docs', 'assets', `readme-banner-${theme}.png`), scale: 2 },
]

const server = await createServer({ logLevel: 'warn', root, server: { host: '127.0.0.1', port: 0 } })
await server.listen()
const base = server.resolvedUrls!.local[0]

// NOTICE: same GPU flags as scripts/export-video.ts; the software path draws
// the veil blur too, just slower.
const browser = await chromium.launch({ args: ['--enable-gpu', '--use-angle=default', '--ignore-gpu-blocklist'] })
try {
  for (const job of JOBS) {
    for (const theme of ['light', 'dark']) {
      const page = await browser.newPage({ deviceScaleFactor: job.scale, viewport: { height: 1080, width: 1920 } })
      await page.goto(`${base}card?kind=${job.kind}&theme=${theme}`)
      await page.locator('#card canvas').waitFor()
      await page.evaluate(() => document.fonts.ready)
      // Let fonts, the GL desk, and backdrop blur settle before the shot.
      await page.waitForTimeout(1200)
      const out = job.out(theme)
      await mkdir(dirname(out), { recursive: true })
      await page.locator('#card').screenshot({ omitBackground: true, path: out })
      console.info(out)
      await page.close()
    }
  }
}
finally {
  await browser.close()
  await server.close()
}
