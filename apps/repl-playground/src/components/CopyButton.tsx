import { useState } from 'react'

import { copyText } from '../clipboard'

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
      <span className={`text-[13px] ${copied ? 'i-ph-check text-good' : 'i-ph-copy'}`} />
    </button>
  )
}
