import config from '@auv/alint-config'

export default [
  {
    name: 'auv/gitignore',
    ignore: {
      gitignore: true,
    },
  },
  {
    name: 'auv/global-ignores',
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
      '**/target/**',
    ],
  },
  config,
]
