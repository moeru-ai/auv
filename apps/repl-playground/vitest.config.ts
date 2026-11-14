import process from 'node:process'

import { playwright } from '@vitest/browser-playwright'
import { defineConfig, mergeConfig } from 'vitest/config'

import viteConfig from './vite.config'

import { browserCommands } from './agent/browser.commands.mjs'

export default mergeConfig(viteConfig, defineConfig({
  // Vitest serves its browser iframe at the root; deployment uses /playground/.
  base: '/',
  test: {
    projects: [
      {
        extends: true,
        test: {
          environment: 'node',
          exclude: ['src/**/*.browser.test.ts'],
          include: ['src/**/*.test.ts'],
          name: 'node',
        },
      },
      {
        extends: true,
        test: {
          browser: {
            commands: browserCommands,
            enabled: true,
            headless: true,
            instances: [{ browser: 'chromium' }],
            provider: playwright({
              actionTimeout: 15000,
              contextOptions: { permissions: ['clipboard-read', 'clipboard-write'] },
              launchOptions: { channel: process.env.PLAYWRIGHT_CHANNEL ?? (process.platform === 'win32' && !process.env.CI ? 'msedge' : undefined) },
            }),
            viewport: { height: 900, width: 1440 },
          },
          include: ['agent/**/*.browser.test.tsx', 'src/**/*.browser.test.tsx'],
          name: 'browser',
          // Browser compilation must not consume the Node tests' import deadlines.
          sequence: { groupOrder: 1 },
          testTimeout: 60000,
        },
      },
    ],
  },
}))
