import type { CodeHeaderProps } from '@assistant-ui/react-markdown'

import remarkGfm from 'remark-gfm'

import { MarkdownTextPrimitive, memoizeMarkdownComponents } from '@assistant-ui/react-markdown'
import { useEffect, useState } from 'react'

import { copyText } from '../../clipboard'
import { IconButton } from '../../components/IconButton'

import '@assistant-ui/react-markdown/styles/dot.css'
import './markdown.css'

function CodeHeader({ code, language }: CodeHeaderProps) {
  const [copied, setCopied] = useState(false)
  useEffect(() => {
    if (!copied)
      return
    const timer = setTimeout(setCopied, 3000, false)
    return () => clearTimeout(timer)
  }, [copied])
  return (
    <div className="text-[11px] text-fg-muted mt-2.5 pb-0.5 pl-2.5 pr-1.5 pt-0.5 border border-line rounded-t-md bg-surface-2 flex items-center justify-between" data-code-header="">
      <span>{language}</span>
      <IconButton
        hint={copied ? 'Copied' : 'Copy code'}
        icon={copied ? 'i-ph-check' : 'i-ph-copy'}
        onClick={() => void copyText(code).then(() => setCopied(true))}
      />
    </div>
  )
}

const components = memoizeMarkdownComponents({
  CodeHeader,
  table: props => <div className="my-2.5 max-w-full overflow-x-auto"><table {...props} /></div>,
})

// assistant-ui's MarkdownText composition, using the existing theme and clipboard:
// https://github.com/assistant-ui/assistant-ui/blob/main/packages/ui/src/components/react/assistant-ui/elements/markdown-text.tsx
export function MarkdownText({ className }: { className: string }) {
  return <MarkdownTextPrimitive className={`agent-md min-w-0 [overflow-wrap:anywhere] ${className}`} components={components} defer remarkPlugins={[remarkGfm]} />
}
