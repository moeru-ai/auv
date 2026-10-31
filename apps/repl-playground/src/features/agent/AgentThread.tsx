import type { ReasoningMessagePartComponent, TextMessagePartComponent } from '@assistant-ui/react'

import { AuiIf, ComposerPrimitive, groupPartByType, MessagePrimitive, ThreadPrimitive, useAuiState } from '@assistant-ui/react'
import { useAcpPermissions, useAcpRuntimeExtras } from '@hafbit/react-acp'
import { useState } from 'react'

import { iconButtonClassName } from '../../components/icon-button-class'
import { IconButton } from '../../components/IconButton'
import { AgentActivity, AgentTool } from './AgentActivity'
import { MarkdownText } from './MarkdownText'

/**
 * Chat over the live ACP connection: assistant messages, tool-call rows,
 * permission approvals, a composer, and new sessions. Codemode returns run
 * results directly to the agent. Run and reset also live in the editor and
 * the activity bar. Styling uses the playground's UnoCSS theme;
 * the session protocol comes from
 * `@hafbit/react-acp` and `@assistant-ui/react`, not hand-rolled.
 */
export function AgentThread() {
  return (
    <ThreadPrimitive.Root className="flex flex-col h-full min-h-0 min-w-0">
      <ApprovalList />
      <div className="flex-1 min-h-0 relative">
        <ThreadPrimitive.Viewport className="h-full [scrollbar-gutter:stable] overflow-x-hidden overflow-y-auto">
          <ThreadPrimitive.Empty>
            <div className="text-[13px] text-fg-muted leading-[1.6] px-5 pb-2 pt-7 flex flex-col gap-2.5 items-start">
              <span aria-hidden="true" className="i-ph-cursor-click text-[22px] text-accent" />
              <strong className="text-[16px] text-fg">Work with your desktop</strong>
              <p className="m-0 max-w-[36ch]">Describe a task. Follow the agent's progress and inspect its results here.</p>
            </div>
          </ThreadPrimitive.Empty>
          <div className="p-4 pb-5 flex flex-col gap-5">
            <ThreadPrimitive.Messages>{() => <AgentMessage />}</ThreadPrimitive.Messages>
          </div>
        </ThreadPrimitive.Viewport>
        <ThreadPrimitive.ScrollToBottom aria-label="Jump to latest message" className="text-[11px] text-fg-muted px-2.5 py-1.25 border border-line rounded-[20px] bg-surface-0 flex gap-1.5 cursor-pointer shadow-[0_2px_8px_#0002] items-center bottom-3 left-1/2 absolute disabled:hidden -translate-x-1/2" type="button">
          <span className="i-ph-arrow-down" />
          Latest
        </ThreadPrimitive.ScrollToBottom>
      </div>
      <div className="px-3 pb-1.25 pt-2 border-t border-line shrink-0">
        <ComposerPrimitive.Root className="p-2 pl-3 border border-line rounded-[10px] bg-surface-1 flex gap-2 items-end focus-within:border-accent">
          <ComposerPrimitive.Input
            aria-label="Message the agent"
            className="text-[13px] text-fg leading-[1.5] px-0 py-0.75 outline-none bg-transparent flex-1 min-w-0 resize-none placeholder:text-fg-subtle"
            maxRows={4}
            placeholder="What would you like to do on this desktop?"
          />
          <AuiIf condition={s => !s.composer.canCancel}>
            <ComposerPrimitive.Send aria-label="Send" className={iconButtonClassName({ tone: 'primary' })}>
              <span className="i-ph-paper-plane-tilt-fill text-[15px]" />
            </ComposerPrimitive.Send>
          </AuiIf>
          <AuiIf condition={s => s.composer.canCancel}>
            <ComposerPrimitive.Cancel aria-label="Stop" className={iconButtonClassName({})}>
              <span className="i-ph-stop-fill text-[15px]" />
            </ComposerPrimitive.Cancel>
          </AuiIf>
        </ComposerPrimitive.Root>
        <div className="text-[10.5px] text-fg-subtle pt-0.75 flex gap-2 min-w-0 items-center">
          <span className="inline-flex gap-1.25 items-center">
            <span className="rounded-full bg-good size-1.25" />
            Connected
          </span>
          <span className="ml-auto">Enter to send</span>
          <AgentToolbar />
        </div>
      </div>
    </ThreadPrimitive.Root>
  )
}

const AgentText: TextMessagePartComponent = ({ text }) => {
  if (!text)
    return <span className="text-fg-subtle animate-pulse">●</span>
  return <MarkdownText className="text-[13px] text-fg leading-[1.75]" />
}

const AgentReasoning: ReasoningMessagePartComponent = ({ text }) => {
  if (!text)
    return null
  return (
    <details className="text-[12px] text-fg-muted">
      <summary className="cursor-pointer">Reasoning</summary>
      <MarkdownText className="leading-relaxed mt-1" />
    </details>
  )
}

const UserText: TextMessagePartComponent = ({ text }) => (
  <div className="flex justify-end">
    <div className="text-[13px] leading-[1.65] px-3.5 py-2.5 rounded-[12px_12px_3px_12px] bg-surface-2 max-w-[92%] whitespace-pre-wrap [overflow-wrap:anywhere]">{text}</div>
  </div>
)

function AgentMessage() {
  const role = useAuiState(s => s.message.role)
  if (role === 'user')
    return <MessagePrimitive.Parts components={{ Text: UserText }} />
  return (
    <div className="min-w-0">
      <MessagePrimitive.GroupedParts groupBy={groupPartByType({ 'tool-call': ['group-activity'] })} indicator="empty">
        {({ children, part }) => {
          switch (part.type) {
            case 'data': return part.dataRendererUI
            case 'group-activity': return <AgentActivity indices={part.indices}>{children}</AgentActivity>
            case 'image': return <img alt="Agent image" className="max-w-full" src={part.image} />
            case 'indicator': return (
              <div aria-live="polite" className="text-[12px] text-fg-subtle py-2 flex gap-2 items-center">
                <span className="i-ph-circle-notch animate-spin" />
                Thinking…
              </div>
            )
            case 'reasoning': return <AgentReasoning {...part} />
            case 'text': return <AgentText {...part} />
            case 'tool-call': return part.toolUI ?? <AgentTool {...part} />
            default: return null
          }
        }}
      </MessagePrimitive.GroupedParts>
    </div>
  )
}

/** Start a new session on the existing connection. */
function AgentToolbar() {
  const extras = useAcpRuntimeExtras()
  const [busy, setBusy] = useState(false)
  const [notice, setNotice] = useState('')

  const newSession = () => {
    if (busy)
      return
    setBusy(true)
    setNotice('')
    // TODO(agent-session-list): session disposal requires a host close
    // capability; add it with an approved session-lifecycle contract.
    void extras.createSession()
      .catch((cause: unknown) => setNotice(cause instanceof Error ? cause.message : String(cause)))
      .finally(() => setBusy(false))
  }

  return (
    <>
      {notice && <span className="text-[11.5px] text-bad flex-1 truncate" role="alert">{notice}</span>}
      <IconButton disabled={busy} hint="Start a new agent session" icon="i-ph-plus" label="New session" onClick={newSession} />
    </>
  )
}

/**
 * Tool permission requests, forwarded unchanged by the relay. Nothing is
 * approved automatically. Codemode reaches the selected Playground device;
 * the agent's other tools may request separate permission here.
 */
function ApprovalList() {
  const { pending, reply } = useAcpPermissions()
  if (pending.length === 0)
    return null
  return (
    <div className="p-1.5 border-b border-line flex shrink-0 flex-col gap-1">
      {pending.map(({ request }) => {
        const { toolCallId } = request.toolCall
        const allowOnce = request.options.find(option => option.kind === 'allow_once')
        const allowAlways = request.options.find(option => option.kind === 'allow_always')
        const reject = request.options.find(option => option.kind === 'reject_once')
          ?? request.options.find(option => option.kind === 'reject_always')
        return (
          <div
            className="px-2 py-1 border border-warn/40 rounded-md bg-surface-0 flex gap-1.5 items-center"
            key={toolCallId}
            title="Needs your confirmation. The agent's local tools run on its own machine."
          >
            <span className="text-[12px] text-fg font-mono flex-1 truncate">{request.toolCall.title ?? 'Tool call'}</span>
            {allowOnce && <button className="btn-primary text-[11.5px] px-2 h-6" onClick={() => void reply(toolCallId, allowOnce.optionId)} type="button">Allow</button>}
            {allowAlways && <button className="btn-ghost text-[11.5px] px-2 border border-line h-6" onClick={() => void reply(toolCallId, allowAlways.optionId)} type="button">Always</button>}
            <button className="btn-ghost text-[11.5px] px-2 h-6" onClick={() => void reply(toolCallId, reject?.optionId)} type="button">Deny</button>
          </div>
        )
      })}
    </div>
  )
}
