import type { Plugin } from 'vite'

import react from '@vitejs/plugin-react'
import UnoCSS from 'unocss/vite'

import { defineConfig } from 'vite'

import { codeModeHelpPlugin } from './agent/help-catalog'

// NOTICE(oauth-callback-page): Cloudflare serves this file at
// `/playground/oauth/callback` and keeps that address. A missing path falls
// back to the docs index, and rewriting it onto `/playground/index.html`
// redirects to `/playground/` before Exterior can redeem the code.
function oauthCallbackPage(): Plugin {
  return {
    apply: 'build',
    // Run after Vite emits index.html so both routes use the same built entry.
    enforce: 'post',
    generateBundle(_, bundle) {
      const index = bundle['index.html']
      if (index?.type !== 'asset')
        this.error('The Playground build must emit index.html for the OAuth callback.')
      this.emitFile({ fileName: 'oauth/callback.html', source: index.source, type: 'asset' })
    },
    name: 'oauth-callback-page',
  }
}

export default defineConfig({
  base: '/playground/',
  // NOTICE(worker-prebundle): discovering these imports on the first worker
  // execution reloads the page and disconnects its ACP tool call. Include
  // compiler and VM dependencies before the first execution starts.
  optimizeDeps: {
    include: ['@jitl/quickjs-singlefile-browser-release-sync', '@moeru/eventa/adapters/webworkers/worker', '@typescript/vfs', 'acorn', 'magic-string', 'quickjs-emscripten-core', 'ts-blank-space', 'typescript'],
  },
  plugins: [codeModeHelpPlugin(), UnoCSS(), react(), oauthCallbackPage()],
  server: {
    port: 5180,
  },
  // Vitest runs in Node, where `@auv-js/sdk` would resolve its Node entry. The
  // REPL runs in the browser, so its tests import the same web entry.
  ssr: {
    resolve: {
      conditions: ['browser'],
    },
  },
  // Workers use dynamic imports (TypeScript lib chunks), so they must be ES modules.
  worker: {
    format: 'es',
    plugins: () => [codeModeHelpPlugin()],
  },
})
