import slop from 'eslint-plugin-slop'

import { defineConfig } from '@moeru/eslint-config'

export default defineConfig({
  masknet: false,
  perfectionist: true,
  preferArrow: false,
  react: true,
  sonarjs: false,
  sortPackageJsonScripts: false,
  typescript: true,
  unocss: false,
  vue: false,
}, {
  ignores: [
    'cspell.config.yaml',
    'cspell.config.yml',
    '**/drizzle/**',
    '**/.astro/**',
    'docs/**',
    '.agents/**',
    '.worktrees/**',
    '.github/**',
    '.cursor/**',
    '.qodana/**',
    'docs/superpowers/**',
    'CLAUDE.md', // Skip the symbolic link
    'qodana.yaml',
    '**/target/',
    '**/.auv/**',
    '**/gen/**',
    'js/packages/cli/binding.d.ts',
    'js/packages/cli/binding.js',
    'proto/**',
    'crates/**',
    'supported/**',
    'devtools/**',
    'bucket/**',
  ],
}, {
  files: ['**/*.{js,mjs,cjs,jsx,ts,mts,cts,tsx,vue}'],
  name: 'auv/slop',
  plugins: { slop },
  rules: {
    'slop/max-comment-length': 'warn',
    'slop/no-chained-type-assertions': 'warn',
    // Product strings can use this punctuation.
    'slop/no-em-dash': 'off',
    'slop/no-jargon': 'warn',
    'slop/no-static-only-class': 'warn',
    'slop/no-trivial-functions': 'warn',
    // Named primitive aliases can express domain concepts.
    'slop/no-trivial-type-aliases': 'off',
    // The autofix workflow must preserve existing comment syntax.
    'slop/prefer-jsdoc': 'off',
  },
  // Full inspection keeps editor and CI results independent of Git history.
  settings: { slop: { inspection: { mode: 'full' } } },
}, {
  rules: {
    'antfu/import-dedupe': 'error',
    // TODO: remove this
    'depend/ban-dependencies': 'warn',
    'import/order': 'off',
    'markdown/require-alt-text': 'off',

    'no-console': ['error', { allow: ['warn', 'error', 'info'] }],
    'no-restricted-syntax': [
      'error',
      // Catches `error instanceof Error ? error.message : ...`. The consequent
      // must be `<x>.message`, so `error instanceof Error ? error : new Error(...)`
      // is not flagged. Antfu's default patterns stay alongside.
      {
        message: 'Avoid `error instanceof Error ? error.message : ...`. Use `errorMessageFrom(error)` from \'@moeru/std\'. Pair with `?? \'fallback\'` when a default is needed.',
        selector: 'ConditionalExpression[test.type=\'BinaryExpression\'][test.operator=\'instanceof\'][test.right.name=\'Error\'][consequent.type=\'MemberExpression\'][consequent.property.name=\'message\']',
      },
      {
        message: 'Avoid hand-written clamp logic. Use `clamp(value, lower, upper)` from `es-toolkit` instead.',
        selector: 'FunctionDeclaration[id.name=/clamp/i] ReturnStatement CallExpression[callee.object.name=\'Math\'][callee.property.name=\'min\'] > CallExpression[callee.object.name=\'Math\'][callee.property.name=\'max\']:first-child',
      },
      {
        message: 'Avoid hand-written clamp logic. Use `clamp(value, lower, upper)` from `es-toolkit` instead.',
        selector: 'FunctionDeclaration[id.name=/clamp/i] ReturnStatement CallExpression[callee.object.name=\'Math\'][callee.property.name=\'max\'] > CallExpression[callee.object.name=\'Math\'][callee.property.name=\'min\']:first-child',
      },
      {
        message: 'Do not use namespace imports from `valibot`. Import the used Valibot APIs by name instead.',
        selector: 'ImportDeclaration[source.value=\'valibot\'] ImportNamespaceSpecifier',
      },
      {
        message: 'Omit TypeScript and JavaScript source extensions from relative imports, dynamic imports, and re-exports.',
        selector: [
          'ImportDeclaration[source.value=/^\\.{1,2}\\/.*\\.[cm]?[jt]sx?$/]',
          'ExportNamedDeclaration[source.value=/^\\.{1,2}\\/.*\\.[cm]?[jt]sx?$/]',
          'ExportAllDeclaration[source.value=/^\\.{1,2}\\/.*\\.[cm]?[jt]sx?$/]',
          'ImportExpression[source.value=/^\\.{1,2}\\/.*\\.[cm]?[jt]sx?$/]',
        ].join(', '),
      },
      'TSEnumDeclaration[const=true]',
      'TSExportAssignment',
    ],
    'style/padding-line-between-statements': 'error',
    'vue/prefer-separate-static-class': 'off',
    'yaml/plain-scalar': 'off',
  },
}, {
  files: ['apps/docs-website/**/*.{ts,tsx}'],
  rules: {
    // Scene drawing modules export their constants (fonts, colors, layout
    // sizes) next to the components that use them. Fast refresh falls back to
    // a full reload for those files, which is fine for this site.
    'react-refresh/only-export-components': 'off',
  },
}, {
  files: ['apps/server/**/*.ts'],
  rules: {
    'no-restricted-syntax': [
      'error',
      {
        message: 'Do not mock internal project modules with vi.mock or vi.doMock. Inject the collaborator through the route, service, or factory boundary and pass a fake or spy in tests.',
        selector: 'CallExpression[callee.type=\'MemberExpression\'][callee.object.name=\'vi\'][callee.property.name=/^(mock|doMock)$/][arguments.0.type=\'Literal\'][arguments.0.value=/^(\\.|@proj-airi\\/|~)/]',
      },
      {
        message: 'Do not use vi.hoisted. If a test needs a collaborator spy, expose an explicit dependency injection point instead of hoisting module mocks.',
        selector: 'CallExpression[callee.type=\'MemberExpression\'][callee.object.name=\'vi\'][callee.property.name=\'hoisted\']',
      },
    ],
  },
}, {
  ignores: [
    '**/*.md',
  ],
  rules: {
    'perfectionist/sort-imports': [
      'error',
      {
        groups: [
          'type-builtin',
          'type-import',
          'type-internal',
          ['type-parent', 'type-sibling', 'type-index'],
          'default-value-builtin',
          'named-value-builtin',
          'value-builtin',
          'default-value-external',
          'named-value-external',
          'value-external',
          'default-value-internal',
          'named-value-internal',
          'value-internal',
          ['default-value-parent', 'default-value-sibling', 'default-value-index'],
          ['named-value-parent', 'named-value-sibling', 'named-value-index'],
          ['wildcard-value-parent', 'wildcard-value-sibling', 'wildcard-value-index'],
          ['value-parent', 'value-sibling', 'value-index'],
          'side-effect',
          'style',
        ],
        newlinesBetween: 1,
      },
    ],
  },
})
