import { Tooltip } from '@base-ui/react/tooltip'
import { useState } from 'react'

import { copyText } from '../clipboard'

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

/** Small copy icon that turns into a check for a moment after copying. */
export function CopyButton({ className = '', label = 'Copy', text }: { className?: string, label?: string, text: string }) {
  const [copied, setCopied] = useState(false)
  return (
    <button
      aria-label={copied ? 'Copied' : label}
      className={`text-fg-subtle rounded flex shrink-0 size-6 cursor-pointer transition-colors items-center justify-center hover:(text-fg bg-surface-2) ${className}`}
      onClick={(event) => {
        event.stopPropagation()
        void copyText(text).then(() => {
          setCopied(true)
          window.setTimeout(setCopied, 1500, false)
        })
      }}
      title={copied ? 'Copied' : label}
      type="button"
    >
      <span className={`text-[13px] ${copied ? 'i-lucide-check text-good' : 'i-lucide-copy'}`} />
    </button>
  )
}

/** Icon-only button with a tooltip; used by the activity bar and the debug bar. */
export function IconButton({ active, disabled, hint, icon, keys, label, onClick, side = 'top', tone = 'ghost' }: {
  active?: boolean
  disabled?: boolean
  hint: string
  icon: string
  keys?: string
  label?: string
  onClick: () => void
  side?: 'bottom' | 'left' | 'right' | 'top'
  tone?: 'ghost' | 'primary'
}) {
  const base = label ? 'h-7 gap-1.5 px-2.5' : 'size-7 justify-center'
  const style = tone === 'primary'
    ? 'bg-accent text-white hover:bg-accent-strong'
    : active ? 'bg-accent/18 text-accent' : 'text-fg-muted hover:(bg-surface-2 text-fg)'
  return (
    <Tooltip.Root>
      <Tooltip.Trigger
        render={(
          <button
            aria-label={hint}
            className={`text-[12.5px] font-medium rounded-md inline-flex cursor-pointer transition-colors items-center disabled:(opacity-35 cursor-not-allowed) ${base}  ${style}`}
            disabled={disabled}
            onClick={onClick}
            type="button"
          />
        )}
      >
        <span className={`${icon} text-[15px]`} />
        {label}
      </Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Positioner side={side} sideOffset={8}>
          <Tooltip.Popup className="text-[12px] text-fg px-2 py-1 border border-line rounded-md bg-surface-2 shadow-lg z-50">
            {hint}
            {keys && <Kbd className="ml-2">{keys}</Kbd>}
          </Tooltip.Popup>
        </Tooltip.Positioner>
      </Tooltip.Portal>
    </Tooltip.Root>
  )
}

export function Kbd({ children, className = '' }: { children: React.ReactNode, className?: string }) {
  return <kbd className={`text-[11px] text-fg-muted font-mono px-1 border border-line rounded bg-surface-2 ${className}`}>{children}</kbd>
}
