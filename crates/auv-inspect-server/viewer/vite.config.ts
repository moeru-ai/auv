import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

export default defineConfig({
  plugins: [vue()],
  base: "/viewer-assets/",
  build: {
    outDir: "dist",
    emptyOutDir: true,
    assetsDir: "assets",
    rollupOptions: {
      output: {
        entryFileNames: "assets/viewer.js",
        chunkFileNames: "assets/[name].js",
        assetFileNames: "assets/[name][extname]"
      }
    }
  },
  server: {
    proxy: {
      "/runs": "http://127.0.0.1:8765",
      "/write": "http://127.0.0.1:8765",
      "/assets": "http://127.0.0.1:8765"
    }
  }
});
