import { defineConfig } from 'tsdown'

export default defineConfig({
  deps: {
    neverBundle: [/^\.\.\/binding\.js$/u],
  },
  dts: true,
  entry: {
    binary: 'src-js/binary.ts',
    index: 'src-js/index.ts',
  },
  fixedExtension: false,
  format: 'esm',
  platform: 'node',
  sourcemap: true,
  target: 'node20',
  unbundle: true,
})
