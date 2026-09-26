import { defineConfig } from 'electron-vite'

export default defineConfig({
  main: {
    build: {
      lib: {
        entry: {
          focus: 'electron/focus.ts',
          keyboard: 'electron/keyboard.ts',
        },
      },
      outDir: 'dist/electron',
    },
  },
  // NOTICE: These receivers load the shared web page from the Rust task's HTTP
  // server. Add preload or a local renderer only when an evaluation needs one.
})
