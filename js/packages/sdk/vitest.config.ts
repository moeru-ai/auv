import { playwright } from '@vitest/browser-playwright'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  test: {
    projects: [
      {
        test: {
          environment: 'node',
          exclude: [
            'src/**/*.browser.test.ts',
            'src/**/*.jsdom.test.ts',
          ],
          globalSetup: ['src/tutils/auv-build.ts'],
          include: ['src/**/*.test.ts'],
          name: 'node',
        },
      },
      {
        test: {
          environment: 'jsdom',
          include: ['src/**/*.jsdom.test.ts'],
          name: 'jsdom',
        },
      },
      {
        test: {
          browser: {
            enabled: true,
            headless: true,
            instances: [{ browser: 'chromium' }],
            provider: playwright(),
          },
          globalSetup: [
            'src/tutils/auv-build.ts',
            'src/tutils/auv-browser-daemon.ts',
          ],
          include: ['src/**/*.browser.test.ts'],
          name: 'browser',
        },
      },
    ],
  },
})
