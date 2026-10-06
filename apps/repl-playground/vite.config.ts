import { fileURLToPath } from 'node:url'

import react from '@vitejs/plugin-react'
import UnoCSS from 'unocss/vite'

import { defineConfig } from 'vite'

export default defineConfig({
  base: '/playground/',
  plugins: [UnoCSS(), react()],
  resolve: {
    alias: {
      // NOTICE(sdk-source-alias): `@auv-js/sdk` exports its built `dist`, but the
      // REPL should always run against the SDK in this repository without a
      // prior build (CI type-checks and tests without building). Mirrors the
      // `paths` entry in tsconfig.json.
      '@auv-js/sdk': fileURLToPath(new URL('../../js/packages/sdk/src/web/index.ts', import.meta.url)),
    },
  },
  server: {
    port: 5180,
  },
  // Workers use dynamic imports (TypeScript lib chunks), so they must be ES modules.
  worker: {
    format: 'es',
  },
})
