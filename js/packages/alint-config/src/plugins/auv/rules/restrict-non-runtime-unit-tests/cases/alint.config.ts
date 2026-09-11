import { createApeiraAdapter } from '@alint-js/agent-apeira'
import { defineConfig } from '@alint-js/plugin'

import auv from '../../../index'

export default defineConfig([
  {
    agent: createApeiraAdapter(),
    files: ['**/*.rs'],
    language: 'text/plain',
    plugins: { rust: auv },
    rules: { 'rust/restrict-non-runtime-unit-tests': 'error' },
  },
])
