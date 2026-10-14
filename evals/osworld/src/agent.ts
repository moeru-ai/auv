import { sleepWithAbort } from '@moeru/std'

export interface AgentResult {
  /** `DONE` or `FAIL` when the model ended the episode, as OSWorld expects. */
  lastAction?: 'DONE' | 'FAIL'
  /** Model replies received, including retries' final successes only. */
  modelRequests?: number
  runIds: string[]
  /** One entry per model reply, kept as debugging evidence. */
  steps: AgentStep[]
  /** Summed `usage` of every model reply; see {@link usageOf}. */
  usage?: Record<string, number>
}

export interface AgentStep {
  arguments?: string
  /** Assistant text that came with the call, if any. */
  message?: string
  /** Tool output text shown to the model, truncated for the result file. */
  output?: string
  runId?: string
  tool?: string
}

/**
 * Observes each model request of {@link runAgent}, for example to emit
 * Vieval `inference.*` events. Calls are best-effort reporting only.
 */
export interface AgentTelemetry {
  failed: (error: unknown) => void
  requested: (step: number) => void
  responded: (reply: Reply, latencyMs: number) => void
}

/** The tools an episode exposes, and how to execute them. */
export interface AgentTools {
  call: (call: FunctionCall, signal: AbortSignal) => Promise<ToolOutput>
  definitions: FunctionTool[]
}

/**
 * One conversation item, independent of the wire protocol. The shape follows
 * the Responses API; {@link requestFor} maps it onto Chat Completions.
 */
export type ConversationItem
  = | (FunctionCall & { type: 'function_call' })
    | { callId: string, images: string[], text: string, type: 'function_call_output' }
    | { image?: string, role: 'user', text: string, type: 'message' }
    | { role: 'assistant', text: string, type: 'message' }

/** One function call chosen by the model. */
export interface FunctionCall {
  /** Raw JSON arguments, exactly as the model produced them. */
  arguments: string
  callId: string
  name: string
}

/** One function the model may call, in the shape both wire APIs accept. */
export interface FunctionTool {
  description?: string
  name: string
  parameters: Record<string, unknown>
}

/**
 * OpenAI-compatible wire protocol: `responses` (`POST /responses`, default) or
 * `chat` (`POST /chat/completions`) for endpoints without the Responses API.
 */
export type ModelApi = 'chat' | 'responses'

/** OpenAI-compatible model endpoint used to choose the next AUV call. */
export interface ModelEndpoint {
  api: ModelApi
  apiKey: string
  baseURL: string
  model: string
}

/** One model request. Without tools, the model can only answer in text. */
export interface Prompt {
  instructions?: string
  items: ConversationItem[]
  tools?: FunctionTool[]
}

/** One model reply: its text and the function calls it chose. */
export interface Reply {
  calls: FunctionCall[]
  text: string
  /** Token usage the endpoint reported, flattened by {@link usageOf}. */
  usage?: Record<string, number>
}

/** What one tool call returns to the model: text plus optional images as data URLs. */
export interface ToolOutput {
  images: string[]
  /** AUV Run that produced the output, when the call reached AUV. */
  runId?: string
  text: string
}

/**
 * A failure inside an agent loop that keeps the steps taken before it, so
 * an episode error (for example a model request rejected by moderation) can
 * still be reviewed.
 */
export class AgentRunError extends Error {
  constructor(readonly partial: AgentResult, cause: unknown) {
    super(`Agent stopped after ${String(partial.steps.length)} steps`, { cause })
    this.name = 'AgentRunError'
  }
}

// NOTICE: One model attempt may not take longer than this. Without it, a
// stalled proxy stream held a case until Vieval's case timeout (p50 6 s,
// p99 314 s, max 832 s in the 2026-10-11 test_nogdrive run). A timed-out
// attempt is retried like a network error.
export const modelAttemptTimeoutMs = 180_000

/** Episode-ending tools. They belong to the eval, not to AUV: OSWorld scores infeasible tasks by `FAIL`. */
export const episodeTools: FunctionTool[] = [
  {
    description: 'Call when the task is complete. Ends the episode.',
    name: 'done',
    parameters: { additionalProperties: false, properties: {}, type: 'object' },
  },
  {
    description: 'Call when the task cannot be completed on this computer: the request is impossible, contradicts itself, or needs a feature, file, or setting the application does not have. Ends the episode.',
    name: 'fail',
    parameters: { additionalProperties: false, properties: {}, type: 'object' },
  },
]

// NOTICE: The 2026-10-12 gpt-5.5 run never called `fail` on infeasible tasks,
// trusted unverified pastes, and pasted terminal scripts into 30 GUI tasks
// (shell skill, not AUV). The prompt names all three; `src/terminalUse.ts`
// counts what remains. Keep it task-agnostic.
export const systemPrompt = `You operate an Ubuntu GNOME desktop through AUV tools to complete the user's task.
Look at the screen before acting: screenshots that AUV captures are attached to the tool result.
Screen coordinates are logical pixels; the display is 1920x1080 at scale 1.
Act with one tool call per reply. Call done when the task is complete, or fail if it cannot be done.
Work in the application the task names, through its own windows, menus, and dialogs. Use a terminal only when the task is about the terminal, the shell, or system state that has no graphical setting; do not write scripts to edit documents, spreadsheets, images, or settings in place of the application.
Some tasks are impossible as asked, for example a feature the application does not have. Decide whether the task is feasible before working around it; if it is not, call fail instead of approximating it. Do not call fail only because a step is slow or tedious.
A completed AUV command means the input was delivered, not that it had the intended effect. Check the screen before you call done.
The user password is "password".`

/**
 * Waits between attempts while the proxy has no capacity (429, 502, 503).
 *
 * NOTICE: cliproxyapi answers 503 `auth_unavailable` while every Codex
 * account cools down, which lasted minutes in the 2026-10-12 run, and relays
 * the upstream's 502 `service_unavailable_error` when the model is
 * overloaded (gpt-6.1-sol at 12 episodes, 2026-10-13). The schedule waits up
 * to about ten minutes before the episode fails.
 */
export const capacityBackoffMs = [15_000, 30_000, 60_000, 120_000, 120_000, 120_000, 120_000]

/**
 * Requests one model reply, retrying transient proxy failures.
 *
 * Network errors, attempts over three minutes, 408, and other 5xx are retried
 * at most three times with a short linear backoff; cliproxyapi answers 408
 * when its upstream stream closes before `response.completed`. 429, 502, and
 * 503 mean no capacity and wait longer, honoring `Retry-After`. Other HTTP errors
 * fail the case immediately.
 *
 * Use when:
 * - the agent loop needs the next decision
 * - `doctor` probes that the configured endpoint answers the same request shape
 */
export async function complete(endpoint: ModelEndpoint, prompt: Prompt, signal: AbortSignal): Promise<Reply> {
  const request = requestFor(endpoint, prompt)
  let transientFailures = 0
  let capacityWaits = 0
  for (;;) {
    const outcome = await fetch(request.url, {
      body: request.body,
      headers: { 'authorization': `Bearer ${endpoint.apiKey}`, 'content-type': 'application/json' },
      method: 'POST',
      signal: AbortSignal.any([signal, AbortSignal.timeout(modelAttemptTimeoutMs)]),
    }).then(async response => response.ok
      ? { reply: replyFrom(endpoint.api, await response.json()) }
      : { response }).catch((error: unknown) => {
      if (transientFailures + 1 >= 3 || signal.aborted) {
        throw error
      }
      return undefined
    })
    if (outcome?.reply !== undefined) {
      return outcome.reply
    }
    const response = outcome?.response
    if (response != null && [429, 502, 503].includes(response.status) && capacityWaits < capacityBackoffMs.length) {
      await sleepWithAbort(retryAfterMs(response) ?? capacityBackoffMs[capacityWaits], signal)
      capacityWaits += 1
      continue
    }
    transientFailures += 1
    if (response != null && (!isTransientStatus(response.status) || transientFailures >= 3)) {
      throw new Error(`Model request to ${request.url} failed with HTTP ${response.status}: ${await response.text()}`)
    }
    await sleepWithAbort(transientFailures * 5_000, signal)
  }
}

/** Reads `OSWORLD_OPENAI_API`; unset or empty selects the Responses API. */
export function modelApiFrom(value: string | undefined): ModelApi {
  if (value == null || value === '' || value === 'responses') {
    return 'responses'
  }
  if (value === 'chat') {
    return 'chat'
  }
  throw new Error(`OSWORLD_OPENAI_API must be "responses" or "chat", got ${JSON.stringify(value)}.`)
}

/**
 * Renders a {@link Prompt} into one wire request. Exported so `doctor` can
 * print the exact body it sent as a reproduce command.
 */
export function requestFor(endpoint: ModelEndpoint, prompt: Prompt): { body: string, url: string } {
  const baseURL = endpoint.baseURL.replace(/\/$/, '')
  const tools = prompt.tools ?? []
  if (endpoint.api === 'chat') {
    const body = {
      messages: chatMessages(prompt),
      model: endpoint.model,
      ...(tools.length === 0
        ? {}
        : {
            parallel_tool_calls: false,
            tools: tools.map(tool => ({ function: tool, type: 'function' })),
          }),
    }
    return { body: JSON.stringify(body), url: `${baseURL}/chat/completions` }
  }
  // NOTICE: `store: false` keeps every step a self-contained request (history
  // is resent, no `previous_response_id`), so a step can be replayed from the
  // episode evidence alone and no conversation state lives at the provider.

  // NOTICE: The Codex backend behind cliproxyapi injects its image_generation
  // tool. With our own functions declared, the model calls them and the turn
  // ends; without any, `tool_choice: 'none'` keeps it from generating images.
  const body = {
    input: prompt.items.map(responsesItem),
    instructions: prompt.instructions,
    model: endpoint.model,
    store: false,
    ...(tools.length === 0
      ? { tool_choice: 'none' }
      : {
          parallel_tool_calls: false,
          tool_choice: 'auto',
          tools: tools.map(tool => ({ type: 'function', ...tool })),
        }),
  }
  return { body: JSON.stringify(body), url: `${baseURL}/responses` }
}

/**
 * Flattens a reply's numeric `usage` fields, one nested level deep, so
 * Responses (`input_tokens`, `input_tokens_details.cached_tokens`,
 * `output_tokens_details.reasoning_tokens`) and Chat Completions
 * (`prompt_tokens`, `completion_tokens`, …) totals can be summed by key.
 * `undefined` when the endpoint reported no usage.
 */
export function usageOf(body: unknown): Record<string, number> | undefined {
  const usage = (body as null | { usage?: unknown })?.usage
  if (usage == null || typeof usage !== 'object') {
    return undefined
  }
  const flat: Record<string, number> = {}
  for (const [key, value] of Object.entries(usage)) {
    if (typeof value === 'number' && Number.isFinite(value)) {
      flat[key] = value
    }
    else if (value != null && typeof value === 'object') {
      for (const [inner, number] of Object.entries(value)) {
        if (typeof number === 'number' && Number.isFinite(number)) {
          flat[`${key}.${inner}`] = number
        }
      }
    }
  }
  return flat
}

function chatMessages(prompt: Prompt): unknown[] {
  const messages: Array<Record<string, unknown>> = prompt.instructions == null ? [] : [{ content: prompt.instructions, role: 'system' }]
  for (const item of prompt.items) {
    if (item.type === 'function_call') {
      const call = { function: { arguments: item.arguments, name: item.name }, id: item.callId, type: 'function' }
      const previous = messages.at(-1)
      if (previous?.role === 'assistant' && previous.tool_calls == null) {
        previous.tool_calls = [call]
      }
      else {
        messages.push({ content: null, role: 'assistant', tool_calls: [call] })
      }
      continue
    }
    if (item.type === 'function_call_output') {
      messages.push({ content: item.text, role: 'tool', tool_call_id: item.callId })
      // Chat `tool` messages carry text only, so images follow as a user turn.
      if (item.images.length > 0) {
        messages.push({ content: [{ text: `Images from ${item.callId}:`, type: 'text' }, ...item.images.map(url => ({ image_url: { url }, type: 'image_url' }))], role: 'user' })
      }
      continue
    }
    if (item.role === 'user' && item.image != null) {
      messages.push({ content: [{ text: item.text, type: 'text' }, { image_url: { url: item.image }, type: 'image_url' }], role: 'user' })
      continue
    }
    messages.push({ content: item.text, role: item.role })
  }
  return messages
}

function isTransientStatus(status: number): boolean {
  return status === 408 || status === 429 || status >= 500
}

/** Extracts the assistant text and function calls; Responses output may also hold reasoning items. */
function replyFrom(api: ModelApi, body: unknown): Reply {
  if (api === 'chat') {
    const message = (body as { choices?: Array<{ message?: { content?: null | string, tool_calls?: Array<{ function: { arguments: string, name: string }, id: string }> } }> }).choices?.[0]?.message
    return {
      calls: (message?.tool_calls ?? []).map(call => ({ arguments: call.function.arguments, callId: call.id, name: call.function.name })),
      text: message?.content ?? '',
      usage: usageOf(body),
    }
  }
  const output = (body as { output?: Array<{ arguments?: string, call_id?: string, content?: Array<{ text?: string, type: string }>, name?: string, type: string }> }).output ?? []
  return {
    calls: output
      .filter(item => item.type === 'function_call')
      .map(item => ({ arguments: item.arguments ?? '{}', callId: item.call_id ?? '', name: item.name ?? '' })),
    text: output
      .filter(item => item.type === 'message')
      .flatMap(item => item.content ?? [])
      .filter(part => part.type === 'output_text')
      .map(part => part.text ?? '')
      .join(''),
    usage: usageOf(body),
  }
}

function responsesItem(item: ConversationItem): unknown {
  switch (item.type) {
    case 'function_call':
      return { arguments: item.arguments, call_id: item.callId, name: item.name, type: 'function_call' }
    case 'function_call_output':
      return {
        call_id: item.callId,
        output: item.images.length === 0
          ? item.text
          : [{ text: item.text, type: 'input_text' }, ...item.images.map(url => ({ image_url: url, type: 'input_image' }))],
        type: 'function_call_output',
      }
    default:
      if (item.role === 'assistant') {
        return { content: [{ text: item.text, type: 'output_text' }], role: 'assistant' }
      }
      return { content: [{ text: item.text, type: 'input_text' }, ...(item.image == null ? [] : [{ image_url: item.image, type: 'input_image' }])], role: 'user' }
  }
}

/** Reads a `Retry-After` header in seconds, bounded to five minutes. */
function retryAfterMs(response: Response): number | undefined {
  const seconds = Number(response.headers.get('retry-after'))
  return Number.isFinite(seconds) && seconds > 0 ? Math.min(seconds, 300) * 1_000 : undefined
}
