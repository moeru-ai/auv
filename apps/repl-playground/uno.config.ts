import { defineConfig, presetIcons, presetWind4, transformerVariantGroup } from 'unocss'

export default defineConfig({
  content: {
    pipeline: {
      // Icon and color classes also live in plain `.ts` tables (e.g. the
      // Device platform icons), which the default pipeline skips.
      include: [/\.[jt]sx?($|\?)/, /\.html($|\?)/],
    },
  },
  presets: [
    presetWind4(),
    presetIcons({ scale: 1.1 }),
  ],
  shortcuts: {
    'btn': 'inline-flex items-center gap-1.5 rounded-md px-2.5 h-7 text-[13px] font-medium transition-colors disabled:(opacity-40 cursor-not-allowed) cursor-pointer select-none',
    'btn-ghost': 'btn text-fg-muted hover:(bg-surface-2 text-fg)',
    'btn-primary': 'btn bg-accent text-white hover:bg-accent-strong',
    'input': 'h-8 w-full rounded-md border border-line bg-surface-1 px-2.5 text-[13px] text-fg outline-none focus:border-accent placeholder:text-fg-subtle',
    'panel-title': 'text-[11px] font-semibold uppercase tracking-wider text-fg-subtle',
  },
  theme: {
    colors: {
      'accent': 'var(--c-accent)',
      'accent-strong': 'var(--c-accent-strong)',
      'bad': 'var(--c-bad)',
      'fg': 'var(--c-fg)',
      'fg-muted': 'var(--c-fg-muted)',
      'fg-subtle': 'var(--c-fg-subtle)',
      'good': 'var(--c-good)',
      'info': 'var(--c-info)',
      'kind': {
        display: 'var(--k-display)',
        frame: 'var(--k-frame)',
        input: 'var(--k-input)',
        text: 'var(--k-text)',
        window: 'var(--k-window)',
      },
      'line': 'var(--c-line)',
      'surface-0': 'var(--c-surface-0)',
      'surface-1': 'var(--c-surface-1)',
      'surface-2': 'var(--c-surface-2)',
      'syn': {
        keyword: 'var(--syn-keyword)',
        number: 'var(--syn-number)',
        string: 'var(--syn-string)',
        type: 'var(--syn-type)',
      },
      'warn': 'var(--c-warn)',
    },
    font: {
      mono: 'var(--font-mono)',
      sans: 'var(--font-sans)',
    },
  },
  transformers: [transformerVariantGroup()],
})
