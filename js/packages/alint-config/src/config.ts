import { createApeiraAdapter } from '@alint-js/agent-apeira'
import { defineConfig } from '@alint-js/plugin'

import auv from './plugins/auv'

export default defineConfig([
  {
    agent: createApeiraAdapter(),
    directories: ['crates/*'],
    files: ['**/*.rs'],
    language: 'plaintext',
    name: 'auv/rust',
    plugins: {
      rust: auv,
    },
    rules: {
      'rust/no-private-schema-toolkit': 'warn',
      'rust/no-unearned-function-boundary': 'warn',
      'rust/no-vacant-control-boundary': 'warn',
      'rust/prefer-established-foundation': 'warn',
    },
  },
  {
    agent: createApeiraAdapter(),
    files: ['**/{src,tests,examples}/**/*.rs'],
    language: 'plaintext',
    name: 'auv/rust-test-contracts',
    plugins: {
      rust: auv,
    },
    rules: {
      'rust/no-mod-names-checks-in-tests': 'error',
      'rust/no-source-files-compare-in-tests': 'error',
    },
  },
  {
    agent: createApeiraAdapter(),
    files: [
      '**/{src,examples}/**/*.rs',
    ],
    language: 'plaintext',
    name: 'auv/side-by-side-rust-unit-tests',
    plugins: {
      rust: auv,
    },
    rules: {
      'rust/require-side-by-side-unit-tests': 'error',
    },
  },
  {
    agent: createApeiraAdapter(),
    files: [
      'supported/**/{src,tests,examples}/**/*.rs',
    ],
    language: 'plaintext',
    name: 'auv/non-runtime-test-ownership',
    plugins: {
      rust: auv,
    },
    rules: {
      'rust/restrict-non-runtime-unit-tests': 'error',
    },
  },
])
