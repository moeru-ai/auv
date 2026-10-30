/**
 * Button look shared by IconButton and components that cannot render through
 * it, such as assistant-ui's composer send and cancel buttons.
 */
export function iconButtonClassName({ active, label, tone = 'ghost' }: {
  active?: boolean
  label?: string
  tone?: 'ghost' | 'primary'
}): string {
  const base = label ? 'h-7 gap-1.5 px-2.5' : 'size-7 justify-center'
  const style = tone === 'primary'
    ? 'bg-accent text-white hover:bg-accent-strong'
    : active ? 'bg-accent/18 text-accent' : 'text-fg-muted hover:(bg-surface-2 text-fg)'
  return `text-[12.5px] font-medium rounded-md inline-flex cursor-pointer transition-colors items-center disabled:(opacity-35 cursor-not-allowed) ${base} ${style}`
}
