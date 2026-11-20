import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import react from '@vitejs/plugin-react'
import UnoCSS from 'unocss/vite'

import { build } from 'esbuild'
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
  plugins: [UnoCSS(), react(), {
    async load(id) {
      if (id !== '\0auv-agent-sdk')
        return
      const root = dirname(fileURLToPath(import.meta.url))
      const bundle = await build({
        absWorkingDir: root,
        bundle: true,
        entryPoints: ['src/features/agent/sandbox-sdk.ts'],
        format: 'iife',
        globalName: 'sdk',
        inject: ['src/features/agent/sandbox-platform.ts'],
        mainFields: ['module', 'main'],
        metafile: true,
        minify: true,
        platform: 'neutral',
        target: 'es2022',
        write: false,
      })
      for (const input of Object.keys(bundle.metafile.inputs))
        this.addWatchFile(resolve(root, input))
      return `export default ${JSON.stringify(bundle.outputFiles[0]!.text)}`
    },
    name: 'auv-agent-sdk',
    resolveId(id) {
      if (id === 'virtual:auv-agent-sdk')
        return '\0auv-agent-sdk'
    },
  }],
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
