import type { ToolCallContent } from '@agentclientprotocol/sdk'
import type { ToolCallMessagePartComponent } from '@assistant-ui/react'
import type { PropsWithChildren } from 'react'

import { useAuiState } from '@assistant-ui/react'
import { Dialog } from '@base-ui/react/dialog'
import { useAcpSession } from '@hafbit/react-acp'

import { formatValue, presentTool } from './tool-presentation'

/** Adjacent tool activity stays folded; returned errors and images remain visible. */
export function AgentActivity({ children, indices }: PropsWithChildren<{ indices: readonly number[] }>) {
  const parts = useAuiState(s => s.message.content)
  const session = useAcpSession()
  const tools = indices.flatMap((index) => {
    const part = parts[index]
    const tool = part?.type === 'tool-call' ? session?.tools[part.toolCallId] : undefined
    return tool ? [{ ...presentTool(tool.value), id: tool.toolCallId, value: tool.value }] : []
  })
  const errors = tools.filter(tool => tool.error)
  const running = tools.some(tool => tool.running)
  const names = [...new Set(tools.map(tool => tool.title))].join(', ')
  const images = tools.flatMap(tool => (tool.value.content ?? []).flatMap((part, index) => part.type === 'content' && part.content.type === 'image'
    ? [{ id: `${tool.id}:${index}`, image: `data:${part.content.mimeType};base64,${part.content.data}` }]
    : []))
  return (
    <div className="my-2.5 min-w-0" data-agent-activity="">
      <details className="group/activity">
        <summary className="disclosure-summary text-[12px] text-fg-muted py-1.5 list-none flex gap-2 min-w-0 items-center">
          <span aria-hidden="true" className={`text-[15px] shrink-0 ${running ? 'i-ph-circle-notch animate-spin' : 'i-ph-stack-simple'}`} />
          <span className="shrink-0">{running ? 'Working' : `${tools.length} ${tools.length === 1 ? 'activity' : 'activities'}`}</span>
          <span className="text-fg-subtle truncate" title={names}>{names}</span>
          {!!errors.length && <span className="text-[11px] text-bad shrink-0">{`${errors.length} ${errors.length === 1 ? 'issue' : 'issues'}`}</span>}
          <span aria-hidden="true" className="i-ph-caret-right text-[12px] ml-auto shrink-0 group-open/activity:rotate-90" />
        </summary>
        <div className="mb-2 ml-1.75 mt-1 pl-3 border-l border-line">{children}</div>
      </details>
      {!!errors.length && (
        <div className="text-bad mb-2.5 ml-1.75 mt-1.5 px-2.5 border-bad border-l-2">
          {errors.map(tool => <p className="text-[12px] leading-[1.55] my-1 [overflow-wrap:anywhere] line-clamp-2" key={tool.id} title={tool.errorText}>{tool.errorText}</p>)}
        </div>
      )}
      {!!images.length && (
        <div aria-label="Screenshots" className="mt-2 flex flex-wrap gap-2">
          {images.map((item, index) => <AgentScreenshot image={item.image} key={item.id} label={`Screenshot ${index + 1}`} />)}
        </div>
      )}
    </div>
  )
}

/** The execution outcome is separate from the wrapper's original protocol status. */
export const AgentTool: ToolCallMessagePartComponent = ({ argsText, result, status, toolCallId, toolName }) => {
  const tool = useAcpSession()?.tools[toolCallId]?.value
  const presentation = tool ? presentTool(tool) : { error: false, running: status.type === 'running', script: undefined, status: status.type, title: toolName }
  const content = tool?.content?.filter(part => part.type !== 'content' || part.content.type !== 'image')
  return (
    <details className="group/tool min-w-0" data-tool-id={toolCallId} data-tool-status={presentation.status}>
      <summary className={`disclosure-summary text-[12px] py-1.75 list-none flex gap-1.75 items-center ${presentation.error ? 'text-bad' : 'text-fg-muted'}`}>
        <span aria-hidden="true" className={`text-[14px] shrink-0 ${presentation.running ? 'i-ph-circle-notch animate-spin' : presentation.error ? 'i-ph-warning-circle' : 'i-ph-check'}`} />
        <span className="min-w-0 truncate">{presentation.title}</span>
        <span className={`text-[10.5px] shrink-0 ${presentation.error ? 'text-bad' : 'text-fg-subtle'}`}>{presentation.running ? 'Running' : presentation.error ? 'Returned an error' : 'Finished'}</span>
        <span aria-hidden="true" className="i-ph-caret-right text-[12px] ml-auto shrink-0 group-open/tool:rotate-90" />
      </summary>
      <div className="pb-2.5 pt-1 min-w-0">
        {(!!content?.length || (!tool?.content?.length && result !== undefined)) && (
          <div>
            <div className="text-[10.5px] text-fg-subtle mt-1.5">Result</div>
            {content?.length
              ? content.map((part, index) => (
                  // eslint-disable-next-line react/no-array-index-key -- ACP supplies ordered blocks without IDs; this renderer has no local state.
                  <AgentToolContent content={part} key={index} />
                ))
              : <pre className="agent-tool-output">{formatValue(result)}</pre>}
          </div>
        )}
        {presentation.script && (
          <details className="group/detail text-[11px] text-fg-muted mt-2">
            <summary className="disclosure-summary py-0.75 list-inside list-item [list-style-type:disclosure-closed] group-open/detail:[list-style-type:disclosure-open]">Script</summary>
            <pre className="agent-tool-output" data-tool-script="">{presentation.script}</pre>
          </details>
        )}
        <details className="group/detail text-[11px] text-fg-muted mt-2">
          <summary className="disclosure-summary py-0.75 list-inside list-item [list-style-type:disclosure-closed] group-open/detail:[list-style-type:disclosure-open]">Protocol details</summary>
          <div className="text-[10.5px] text-fg-subtle mt-1.5">{`ACP status: ${tool?.status ?? status.type}`}</div>
          <div className="text-[10.5px] text-fg-subtle mt-1.5">Raw input</div>
          <pre className="agent-tool-output">{tool?.rawInput === undefined ? argsText : formatValue(tool.rawInput)}</pre>
          <div className="text-[10.5px] text-fg-subtle mt-1.5">Raw output</div>
          <pre className="agent-tool-output">{formatValue(tool?.rawOutput ?? result)}</pre>
        </details>
      </div>
    </details>
  )
}

/** Preview the bounded image already returned by the agent; opening fetches no extra pixels. */
function AgentScreenshot({ image, label }: { image: string, label: string }) {
  return (
    <Dialog.Root>
      <Dialog.Trigger aria-label={`View ${label.toLowerCase()}`} className="text-fg-muted p-0 text-left border border-line rounded-lg bg-surface-1 flex flex-col max-w-full w-[148px] cursor-zoom-in overflow-hidden focus-visible:(outline-2 outline-accent outline-offset-3 rounded)" type="button">
        <img alt={label} className="h-[92px] w-full block object-contain" loading="lazy" src={image} />
        <span className="text-[10.5px] px-2 py-1.25 border-t border-line flex w-full items-center justify-between">
          {label}
          <span aria-hidden="true" className="i-ph-arrows-out" />
        </span>
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Backdrop className="bg-[#000a] inset-0 fixed z-80" />
        <Dialog.Popup className="text-fg p-2 border border-line rounded-xl bg-surface-0 max-h-[90vh] max-w-[90vw] w-max shadow-[0_20px_80px_#0007] left-1/2 top-1/2 fixed z-81 overflow-auto -translate-1/2">
          <div className="text-[13px] px-1 pb-2 flex gap-6 items-center justify-between">
            <Dialog.Title>{label}</Dialog.Title>
            <Dialog.Close aria-label="Close screenshot" className="btn-ghost" type="button"><span className="i-ph-x" /></Dialog.Close>
          </div>
          <img alt={`${label} preview`} className="max-h-[78vh] max-w-[calc(90vw-18px)] block object-contain" src={image} />
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

/** Preserve other ACP content as readable data instead of dropping it. */
function AgentToolContent({ content }: { content: ToolCallContent }) {
  return <pre className="agent-tool-output">{content.type === 'content' && content.content.type === 'text' ? content.content.text : formatValue(content)}</pre>
}
