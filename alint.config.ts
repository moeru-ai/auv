import config from '@auv-js/alint-config'

export default [
  {
    ignore: {
      gitignore: true,
    },
    name: 'auv/gitignore',
  },
  {
    ignores: [
      '**/.git/**',
      '**/.hg/**',
      '**/.svn/**',
      '**/.codex/**',
      '**/.codex-live-revalidate/**',
      '**/.cursor/**',
      '**/.idea/**',
      '**/.runs/**',
      '**/.superpowers/**',
      '**/.vscode/**',
      '**/.worktrees/**',
      '**/.auv/**',
      '**/AGENTS.md',
      '**/CLAUDE.md',
      '**/GEMINI.md',
      '**/Cargo.lock',
      '**/node_modules/**',
      '**/pnpm-lock.yaml',
      '**/src/gen/**',
      '**/target/**',
    ],
    name: 'auv/global-ignores',
  },
  config,
]
