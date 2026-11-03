import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

import { build } from 'vite'
import { expect, it } from 'vitest'

import config from './vite.config'

// ROOT CAUSE:
//
// If the deployed site received an OAuth callback, it served the docs page
// because Cloudflare's SPA fallback used the root index.
// Before the fix, no Playground callback asset existed.
// The fix emits Playground HTML at the callback path to keep the URL intact.
it('builds the Playground entry at the OAuth callback path', async () => {
  const root = await mkdtemp(join(tmpdir(), 'auv-playground-callback-'))
  try {
    await writeFile(join(root, 'index.html'), '<div id="root"></div><script type="module" src="/main.js"></script>')
    await writeFile(join(root, 'main.js'), 'document.getElementById("root").textContent = "Playground"')
    await writeFile(join(root, 'uno.config.ts'), 'export default {}')
    await build({
      ...config,
      build: { outDir: 'site' },
      configFile: false,
      logLevel: 'silent',
      publicDir: false,
      root,
    })
    const index = await readFile(join(root, 'site/index.html'), 'utf8')
    const callback = await readFile(join(root, 'site/oauth/callback.html'), 'utf8')
    expect(callback).toBe(index)
    expect(callback).toContain('src="/playground/assets/')
  }
  finally {
    await rm(root, { force: true, recursive: true })
  }
}, 30_000)
