import react from '@vitejs/plugin-react'
import UnoCSS from 'unocss/vite'

import { defineConfig } from 'vite'

export default defineConfig({
  base: '/playground/',
  // NOTICE(worker-prebundle): discovering these imports on the first worker
  // execution reloads the page and disconnects its ACP tool call.
  optimizeDeps: {
    include: [
      '@moeru/eventa/adapters/webworkers/worker',
      '@typescript/vfs',
      'acorn',
      'magic-string',
      'ts-blank-space',
      'typescript',
    ],
  },
  plugins: [UnoCSS(), react()],
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
  },
})
