import { defineConfig } from 'tsdown'

export default defineConfig({
  deps: {
    neverBundle: [/^@auv-js\/api-client$/u, /^node:/u],
  },
  dts: {
    sourcemap: true,
  },
  entry: {
    index: 'src/web/index.ts',
    node: 'src/node/index.ts',
  },
  fixedExtension: false,
  format: 'esm',
  platform: 'neutral',
  sourcemap: true,
  target: 'es2022',
})
