import type { ToolCallMessagePartComponent } from '@assistant-ui/react'
import type { PropsWithChildren } from 'react'

import { useAuiState } from '@assistant-ui/react'
import { Collapsible } from '@base-ui/react/collapsible'
import { Dialog } from '@base-ui/react/dialog'
import { useAcpSession } from '@hafbit/react-acp'

import { iconButtonClassName } from '../../components/icon-button-class'
import { formatValue, presentTool } from './tool-presentation'

/** Adjacent tool activity stays folded; returned errors and images remain visible. */
export function AgentActivity({ children, indices }: PropsWithChildren<{ indices: readonly number[] }>) {
  const parts = useAuiState(s => s.message.content)
  const session = useAcpSession()
  const tools = indices.flatMap((index) => {
    const part = parts[index]
    if (part?.type !== 'tool-call')
      return []
    const tool = session?.tools[part.toolCallId]?.value
    return tool ? [{ ...presentTool(tool), content: tool.content, id: part.toolCallId }] : []
  })
  const errors = tools.filter(tool => tool.error)
  const running = tools.some(tool => tool.running)
  const names = [...new Set(tools.map(tool => tool.title))].join(', ')
  const images = tools.flatMap(tool => (tool.content ?? []).flatMap((part, index) => part.type === 'content' && part.content.type === 'image'
    ? [{ id: `${tool.id}:${index}`, image: `data:${part.content.mimeType};base64,${part.content.data}` }]
    : []))
  return (
    <Collapsible.Root className="my-2.5 min-w-0" data-agent-activity="">
      <Collapsible.Trigger className="group text-[12px] text-fg-muted py-1.5 text-left disclosure-summary flex gap-2 min-w-0 w-full items-center">
        <span aria-hidden="true" className={`text-[15px] shrink-0 ${running ? 'i-ph-circle-notch animate-spin' : 'i-ph-stack-simple'}`} />
        <span className="shrink-0">{running ? 'Working' : `${tools.length} ${tools.length === 1 ? 'activity' : 'activities'}`}</span>
        <span className="text-fg-subtle truncate" title={names}>{names}</span>
        {!!errors.length && <span className="text-[11px] text-bad shrink-0">{`${errors.length} ${errors.length === 1 ? 'issue' : 'issues'}`}</span>}
        <span aria-hidden="true" className="i-ph-caret-right text-[12px] ml-auto shrink-0 group-data-[panel-open]:rotate-90" />
      </Collapsible.Trigger>
      <Collapsible.Panel className="mb-2 ml-1.75 mt-1 pl-3 border-l border-line" keepMounted>{children}</Collapsible.Panel>
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
    </Collapsible.Root>
  )
}

export const AgentTool: ToolCallMessagePartComponent = ({ argsText, result, toolCallId, toolName }) => {
  const tool = useAcpSession()?.tools[toolCallId]?.value
  const presentation = tool && presentTool(tool)
  const content = tool?.content?.filter(part => part.type !== 'content' || part.content.type !== 'image')
  return (
    <Collapsible.Root className="min-w-0">
      <Collapsible.Trigger className={`group text-[12px] py-1.75 text-left disclosure-summary flex gap-1.75 w-full items-center ${presentation?.error ? 'text-bad' : 'text-fg-muted'}`}>
        <span aria-hidden="true" className={`text-[14px] shrink-0 ${presentation?.running ? 'i-ph-circle-notch animate-spin' : presentation?.error ? 'i-ph-warning-circle' : 'i-ph-circle'}`} />
        <span className="min-w-0 truncate">{presentation?.title ?? toolName}</span>
        {presentation && <span className={`text-[10.5px] shrink-0 ${presentation.error ? 'text-bad' : 'text-fg-subtle'}`}>{presentation.running ? 'Running' : presentation.error ? 'Returned an error' : 'Finished'}</span>}
        <span aria-hidden="true" className="i-ph-caret-right text-[12px] ml-auto shrink-0 group-data-[panel-open]:rotate-90" />
      </Collapsible.Trigger>
      <Collapsible.Panel className="pb-2.5 pt-1 min-w-0" keepMounted>
        {(!!content?.length || (!tool?.content?.length && result !== undefined)) && (
          <div>
            <div className="text-[10.5px] text-fg-subtle mt-1.5">Result</div>
            {content?.length
              ? content.map((part, index) => (
                  // eslint-disable-next-line react/no-array-index-key -- ACP supplies ordered blocks without IDs; this renderer has no local state.
                  <pre className="agent-tool-output" key={index}>
                    {part.type === 'content' && part.content.type === 'text' ? part.content.text : formatValue(part)}
                  </pre>
                ))
              : <pre className="agent-tool-output">{formatValue(result)}</pre>}
          </div>
        )}
        {presentation?.script && (
          <details className="group/detail text-[11px] text-fg-muted mt-2">
            <summary className="py-0.75 disclosure-summary list-inside list-item [list-style-type:disclosure-closed] group-open/detail:[list-style-type:disclosure-open]">Script</summary>
            <pre className="agent-tool-output">{presentation.script}</pre>
          </details>
        )}
        <details className="group/detail text-[11px] text-fg-muted mt-2">
          <summary className="py-0.75 disclosure-summary list-inside list-item [list-style-type:disclosure-closed] group-open/detail:[list-style-type:disclosure-open]">Protocol details</summary>
          {tool && <div className="text-[10.5px] text-fg-subtle mt-1.5">{`ACP status: ${tool.status ?? 'pending'}`}</div>}
          <div className="text-[10.5px] text-fg-subtle mt-1.5">Raw input</div>
          <pre className="agent-tool-output">{tool?.rawInput === undefined ? argsText : formatValue(tool.rawInput)}</pre>
          <div className="text-[10.5px] text-fg-subtle mt-1.5">Raw output</div>
          <pre className="agent-tool-output">{formatValue(tool?.rawOutput ?? result)}</pre>
        </details>
      </Collapsible.Panel>
    </Collapsible.Root>
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
        <Dialog.Viewport className="grid inset-0 place-items-center fixed z-81">
          <Dialog.Popup className="text-fg p-2 border border-line rounded-xl bg-surface-0 max-h-[90vh] max-w-[90vw] shadow-[0_20px_80px_#0007] overflow-auto">
            <div className="text-[13px] px-1 pb-2 flex gap-6 items-center justify-between">
              <Dialog.Title>{label}</Dialog.Title>
              <Dialog.Close aria-label="Close screenshot" className={iconButtonClassName({})}>
                <span aria-hidden="true" className="i-ph-x text-[15px]" />
              </Dialog.Close>
            </div>
            <img alt={`${label} preview`} className="max-h-[78vh] max-w-[calc(90vw-18px)] block object-contain" src={image} />
          </Dialog.Popup>
        </Dialog.Viewport>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
