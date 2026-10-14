import type { AgentEvent, AgentMessage, AgentTool } from '@earendil-works/pi-agent-core'
import type { AssistantMessage, ImageContent, Model, StreamFunction, TextContent, ToolResultMessage, Usage } from '@earendil-works/pi-ai'

import type { AgentResult, AgentStep, AgentTelemetry, AgentTools, ModelEndpoint, Reply, ToolOutput } from '../../agent'

import { Agent } from '@earendil-works/pi-agent-core'
import { createModels, createProvider } from '@earendil-works/pi-ai'
import { openAICompletionsApi } from '@earendil-works/pi-ai/api/openai-completions.lazy'
import { openAIResponsesApi } from '@earendil-works/pi-ai/api/openai-responses.lazy'
import { sleepWithAbort } from '@moeru/std'

import { AgentRunError, capacityBackoffMs, episodeTools, modelAttemptTimeoutMs, systemPrompt } from '../../agent'

/**
 * How the loop observes the desktop between model turns.
 *
 * - `observeAfterAction`: after an AUV input or app command succeeds, the
 *   harness captures the display through AUV and appends that screenshot to
 *   the same tool result, as OSWorld agents receive an observation after
 *   every action. Without it, 39-42% of a 50-step budget went to explicit
 *   `display.capture` calls (2026-10-13 gpt-6.1-sol run).
 * - `imagesInHistory`: screenshots kept in the transcript sent to the model;
 *   older tool results keep their text. With one image, the model toggled
 *   between two views it could not hold at once (24 of 51 out-of-steps
 *   episodes looped).
 */
export interface ObservationPolicy {
  imagesInHistory: number
  observeAfterAction: boolean
}

export interface RunAgentOptions {
  maxSteps: number
  /**
   * Model request boundary. Defaults to an OpenAI-compatible provider for
   * `endpoint`; tests pass pi-ai's faux provider here.
   */
  model?: { model: Model<string>, streamFn: StreamFunction<string> }
  observation?: ObservationPolicy
  signal: AbortSignal
  telemetry?: AgentTelemetry
}

/** Reads `OSWORLD_OBSERVE_AFTER_ACTION` (default on) and `OSWORLD_IMAGES_IN_HISTORY` (default 3). */
export function observationPolicyFrom(env: NodeJS.ProcessEnv): ObservationPolicy {
  const images = Number(env.OSWORLD_IMAGES_IN_HISTORY ?? 3)
  if (!Number.isInteger(images) || images < 1) {
    throw new Error(`OSWORLD_IMAGES_IN_HISTORY must be a positive integer, got ${JSON.stringify(env.OSWORLD_IMAGES_IN_HISTORY)}.`)
  }
  return { imagesInHistory: images, observeAfterAction: env.OSWORLD_OBSERVE_AFTER_ACTION !== 'false' }
}

/** Commands after which the screen is expected to change, so the harness observes it. */
const actionCommand = /^(?:input|app|window\.(?:click|drag|scroll))\./u

/**
 * Runs the model -> tool loop on `@earendil-works/pi-agent-core`.
 *
 * Call stack:
 *
 * {@link runEpisode}
 *   -> {@link runAgent}
 *     -> pi `Agent.prompt` / `Agent.continue` (`POST {baseURL}/responses` or `/chat/completions`)
 *     -> `invoke` tool -> `tools.call` (AUV MCP `invoke`)
 *     -> `afterToolCall` -> `tools.call(display.capture)` when {@link ObservationPolicy.observeAfterAction}
 *     -> `done` / `fail` (eval-owned, terminate the run)
 *
 * The eval owns the step budget, the nudge after a text-only reply, and the
 * long capacity backoff; pi owns the transcript, tool execution, and request
 * shaping. Returns the AUV Run IDs, recorded steps, usage, and the terminal
 * action for the evaluator.
 */
export async function runAgent(instruction: string, endpoint: ModelEndpoint, tools: AgentTools, options: RunAgentOptions): Promise<AgentResult> {
  const observation = options.observation ?? { imagesInHistory: 3, observeAfterAction: true }
  const result: AgentResult = { modelRequests: 0, runIds: [], steps: [], usage: {} }
  const { model, streamFn } = options.model ?? openAICompatibleModel(endpoint)
  let observations = 0

  const callAuv = async (argumentsJson: string, signal?: AbortSignal): Promise<ToolOutput> =>
    await tools.call({ arguments: argumentsJson, callId: `observe-${observations++}`, name: 'invoke' }, signal ?? options.signal)

  const auvTools: AgentTool[] = tools.definitions.map(definition => ({
    description: definition.description ?? '',
    execute: async (toolCallId, params, signal) => {
      const output = await tools.call({ arguments: JSON.stringify(params), callId: toolCallId, name: definition.name }, signal ?? options.signal)
      return { content: toolContent(output), details: { runId: output.runId, text: output.text } }
    },
    executionMode: 'sequential',
    label: definition.name,
    name: definition.name,
    // pi validates plain JSON Schema as well as TypeBox schemas.
    parameters: definition.parameters as AgentTool['parameters'],
  }))
  const endTools: AgentTool[] = episodeTools.map(definition => ({
    description: definition.description ?? '',
    execute: async () => ({ content: [{ text: 'Episode ended.', type: 'text' }], details: { action: definition.name }, terminate: true }),
    label: definition.name,
    name: definition.name,
    parameters: definition.parameters as AgentTool['parameters'],
  }))

  const agent = new Agent({
    afterToolCall: async ({ args, isError, result: toolResult, toolCall }, signal) => {
      if (!observation.observeAfterAction || isError || toolCall.name !== 'invoke') {
        return undefined
      }
      const commandId = (args as { command_id?: unknown }).command_id
      if (typeof commandId !== 'string' || !actionCommand.test(commandId)) {
        return undefined
      }
      const capture = await callAuv(JSON.stringify({ command_id: 'display.capture', inputs: {} }), signal).catch(() => undefined)
      if (capture == null || capture.images.length === 0) {
        return undefined
      }
      if (capture.runId != null) {
        result.runIds.push(capture.runId)
      }
      return {
        content: [...toolResult.content, { text: 'Screen after this action:', type: 'text' }, ...toolContent({ ...capture, text: '' })],
        details: { ...(toolResult.details as object), observationRunId: capture.runId },
      }
    },
    // NOTICE: pi calls `finishTurn` before it emits `turn_end`, so the turn is
    // recorded here, where the step budget is decided.
    finishTurn: ({ message, toolResults }) => {
      recordTurn(result, message, toolResults)
      return result.steps.length >= options.maxSteps ? { action: 'end' } : undefined
    },
    initialState: { model, systemPrompt, tools: [...auvTools, ...endTools] },
    streamFn: (requestModel, context, streamOptions) =>
      streamFn(requestModel, context, { ...streamOptions, maxRetries: 0, timeoutMs: modelAttemptTimeoutMs }),
    toolExecution: 'sequential',
    transformContext: async messages => windowImages(messages, observation.imagesInHistory),
  })

  let requestStarted = 0
  agent.subscribe((event: AgentEvent) => {
    if (event.type === 'turn_start') {
      requestStarted = performance.now()
      options.telemetry?.requested(result.modelRequests ?? 0)
    }
    if (event.type === 'message_end' && event.message.role === 'assistant') {
      const message = event.message
      if (message.stopReason === 'error') {
        options.telemetry?.failed(new Error(message.errorMessage ?? 'model request failed'))
        return
      }
      result.modelRequests = (result.modelRequests ?? 0) + 1
      const usage = flatUsage(message.usage)
      for (const [key, value] of Object.entries(usage)) {
        result.usage![key] = (result.usage![key] ?? 0) + value
      }
      options.telemetry?.responded(replyOf(message, usage), performance.now() - requestStarted)
    }
  })
  const abort = (): void => agent.abort()
  options.signal.addEventListener('abort', abort, { once: true })

  let capacityWaits = 0
  let transientFailures = 0
  try {
    await agent.prompt(`Task: ${instruction}`)
    for (;;) {
      options.signal.throwIfAborted()
      if (result.lastAction != null || result.steps.length >= options.maxSteps) {
        return result
      }
      const last = agent.state.messages.at(-1)
      if (last?.role === 'assistant' && last.stopReason === 'error') {
        // Drop the failed reply so the next request resends the same context.
        agent.state.messages = agent.state.messages.slice(0, -1)
        const status = errorStatus(last.errorMessage)
        if (status != null && [429, 502, 503].includes(status) && capacityWaits < capacityBackoffMs.length) {
          await sleepWithAbort(capacityBackoffMs[capacityWaits++], options.signal)
        }
        else if ((status == null || status === 408 || status >= 500) && transientFailures < 3) {
          await sleepWithAbort(++transientFailures * 5_000, options.signal)
        }
        else {
          throw new Error(`Model request failed: ${last.errorMessage ?? 'unknown error'}`)
        }
        await agent.continue()
        continue
      }
      // A text-only reply ends a pi run; the episode asks for an action instead.
      agent.followUp({ content: 'Call a tool to act on the computer, or call done or fail.', role: 'user', timestamp: Date.now() })
      await agent.continue()
    }
  }
  catch (error) {
    throw new AgentRunError(result, error)
  }
  finally {
    options.signal.removeEventListener('abort', abort)
  }
}

/** Keeps images only on the latest `keep` tool results; older ones keep their text. */
export function windowImages(messages: AgentMessage[], keep: number): AgentMessage[] {
  let kept = 0
  return messages.toReversed().map((message) => {
    if (message.role !== 'toolResult' || !message.content.some(part => part.type === 'image')) {
      return message
    }
    kept += 1
    if (kept <= keep) {
      return message
    }
    return {
      ...message,
      content: [...message.content.filter(part => part.type !== 'image'), { text: '[Earlier screenshot omitted.]', type: 'text' }],
    } satisfies ToolResultMessage
  }).toReversed()
}

/** Reads an HTTP status out of a pi error message, when it names one. */
function errorStatus(message: string | undefined): number | undefined {
  const match = /\b([45]\d{2})\b/u.exec(message ?? '')
  return match == null ? undefined : Number(match[1])
}

/** Maps pi usage (input excludes cache reads and writes) onto the Responses field names earlier runs recorded. */
function flatUsage(usage: undefined | Usage): Record<string, number> {
  if (usage == null) {
    return {}
  }
  return {
    'input_tokens': usage.input + usage.cacheRead + usage.cacheWrite,
    'input_tokens_details.cache_write_tokens': usage.cacheWrite,
    'input_tokens_details.cached_tokens': usage.cacheRead,
    'output_tokens': usage.output,
    'output_tokens_details.reasoning_tokens': usage.reasoning ?? 0,
    'total_tokens': usage.totalTokens,
  }
}

/** An OpenAI-compatible endpoint (cliproxyapi) as a pi provider with one image-capable model. */
function openAICompatibleModel(endpoint: ModelEndpoint): { model: Model<string>, streamFn: StreamFunction<string> } {
  const api = endpoint.api === 'chat' ? 'openai-completions' : 'openai-responses'
  const baseUrl = endpoint.baseURL.replace(/\/$/u, '')
  const model: Model<string> = {
    api,
    baseUrl,
    contextWindow: 400_000,
    cost: { cacheRead: 0, cacheWrite: 0, input: 0, output: 0 },
    id: endpoint.model,
    input: ['text', 'image'],
    maxTokens: 32_000,
    name: endpoint.model,
    provider: 'osworld-endpoint',
    // NOTICE: Reasoning stays at the endpoint's default, as the previous loop
    // sent no reasoning settings; enabling it changes cost and latency.
    reasoning: false,
  }
  const provider = createProvider({
    api: api === 'openai-completions' ? openAICompletionsApi() : openAIResponsesApi(),
    auth: { apiKey: { name: 'OSWorld endpoint', resolve: async () => ({ auth: { apiKey: endpoint.apiKey } }) } },
    baseUrl,
    id: 'osworld-endpoint',
    models: [model as never],
  })
  const models = createModels()
  models.setProvider(provider)
  return { model, streamFn: models.streamSimple.bind(models) as StreamFunction<string> }
}

/** Appends one recorded step per tool call of a finished turn, or one text-only step. */
function recordTurn(result: AgentResult, message: AssistantMessage, toolResults: ToolResultMessage[]): void {
  if (message.stopReason === 'error' || message.stopReason === 'aborted') {
    return
  }
  const text = message.content.filter((part): part is TextContent => part.type === 'text').map(part => part.text).join('') || undefined
  const calls = message.content.filter(part => part.type === 'toolCall')
  if (calls.length === 0) {
    result.steps.push({ message: text ?? '' })
    return
  }
  for (const call of calls) {
    const toolResult = toolResults.find(item => item.toolCallId === call.id)
    if (call.name === 'done' || call.name === 'fail') {
      result.steps.push({ message: text, tool: call.name })
      result.lastAction = call.name === 'done' ? 'DONE' : 'FAIL'
      continue
    }
    const details = (toolResult?.details ?? {}) as { runId?: string, text?: string }
    if (details.runId != null) {
      result.runIds.push(details.runId)
    }
    const step: AgentStep = {
      arguments: JSON.stringify(call.arguments),
      message: text,
      output: (details.text ?? toolResult?.content.filter((part): part is TextContent => part.type === 'text').map(part => part.text).join('\n') ?? '').slice(0, 2_000),
      runId: details.runId,
      tool: call.name,
    }
    result.steps.push(step)
  }
}

function replyOf(message: AssistantMessage, usage: Record<string, number>): Reply {
  return {
    calls: message.content.filter(part => part.type === 'toolCall').map(call => ({ arguments: JSON.stringify(call.arguments), callId: call.id, name: call.name })),
    text: message.content.filter((part): part is TextContent => part.type === 'text').map(part => part.text).join(''),
    usage,
  }
}

/** Converts an AUV tool output (text plus data-URL images) into pi content blocks. */
function toolContent(output: ToolOutput): (ImageContent | TextContent)[] {
  const images = output.images.flatMap((url): ImageContent[] => {
    const match = /^data:([^;]+);base64,(.*)$/su.exec(url)
    return match == null ? [] : [{ data: match[2], mimeType: match[1], type: 'image' }]
  })
  return [...(output.text === '' ? [] : [{ text: output.text, type: 'text' as const }]), ...images]
}
