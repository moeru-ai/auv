import process from 'node:process'

import { defineConfig } from 'vite'

const apiOrigin = process.env.AUV_EVAL_API_ORIGIN

if (!apiOrigin)
  throw new Error('AUV_EVAL_API_ORIGIN is required for the receipt proxy')

export default defineConfig({
  root: 'web',
  server: {
    host: '127.0.0.1',
    proxy: {
      '/command': apiOrigin,
      '/receipt': apiOrigin,
    },
  },
})
