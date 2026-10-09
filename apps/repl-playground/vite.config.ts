import react from '@vitejs/plugin-react'
import UnoCSS from 'unocss/vite'

import { defineConfig } from 'vite'

export default defineConfig({
  base: '/playground/',
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
