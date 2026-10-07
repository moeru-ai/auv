import { CopyButton } from './CopyButton'

/** A shell command in a code block with a copy button. */
export function CommandSnippet({ command }: { command: string }) {
  return (
    <div className="text-[12px] font-mono pl-2.5 pr-1 border border-line rounded-md bg-surface-0 flex gap-2 min-h-8 items-start">
      <span className="text-fg-subtle py-1.5 select-none">$</span>
      <code className="text-fg py-1.5 flex-1 select-all [overflow-wrap:anywhere]">{command}</code>
      <CopyButton className="mt-0.5" label="Copy command" text={command} />
    </div>
  )
}
