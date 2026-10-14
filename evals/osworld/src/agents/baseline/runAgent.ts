import type { AgentResult, AgentTelemetry, AgentTools, ConversationItem, ModelEndpoint } from '../../agent'

import { AgentRunError, complete, episodeTools, systemPrompt } from '../../agent'

// NOTICE: This is the hand-written Responses/Chat Completions loop the
// 2026-10-12 (gpt-5.5) and 2026-10-13 (gpt-6.1-sol, 221/312 non-proxy) runs
// used. It stays frozen as a sanity-check baseline (`OSWORLD_HARNESS=baseline`)
// for the pi loop in `../pi/`: change it only to keep it compiling against the
// shared contract in `../../agent.ts`, not to improve it.

/**
 * Screenshots kept in the conversation. Older tool outputs keep their text.
 * One lossless 1920x1080 WebP capture is about 1.5 MB, and every step resends
 * the history, so the window stays at the latest image.
 */
const imagesInHistory = 1

/**
 * Runs a bounded model -> tool call loop.
 *
 * Call stack:
 *
 * {@link runEpisode}
 *   -> {@link runAgent}
 *     -> {@link complete} (`POST {baseURL}/responses` or `/chat/completions`)
 *     -> `tools.call` (AUV MCP `invoke`, or the eval's `done`/`fail`)
 *
 * Use when:
 * - a Vieval case needs a real model to drive AUV inside an OSWorld VM
 *
 * Returns:
 * - the AUV Run IDs, the model's calls, and the terminal action for the evaluator
 */
export async function runAgent(
  instruction: string,
  endpoint: ModelEndpoint,
  tools: AgentTools,
  options: { maxSteps: number, signal: AbortSignal, telemetry?: AgentTelemetry },
): Promise<AgentResult> {
  const result: AgentResult = { modelRequests: 0, runIds: [], steps: [], usage: {} }
  const items: ConversationItem[] = [{ role: 'user', text: `Task: ${instruction}`, type: 'message' }]
  const definitions = [...tools.definitions, ...episodeTools]

  try {
    for (let step = 0; step < options.maxSteps; step += 1) {
      options.telemetry?.requested(step)
      const started = performance.now()
      const reply = await complete(endpoint, { instructions: systemPrompt, items: windowImages(items), tools: definitions }, options.signal)
        .catch((error: unknown) => {
          options.telemetry?.failed(error)
          throw error
        })
      result.modelRequests = (result.modelRequests ?? 0) + 1
      for (const [key, value] of Object.entries(reply.usage ?? {})) {
        result.usage![key] = (result.usage![key] ?? 0) + value
      }
      options.telemetry?.responded(reply, performance.now() - started)
      if (reply.text !== '') {
        items.push({ role: 'assistant', text: reply.text, type: 'message' })
      }
      // `parallel_tool_calls: false` asks for one call; anything extra is not
      // executed and is not recorded, so history matches what ran.
      const call = reply.calls[0]
      if (call == null) {
        result.steps.push({ message: reply.text })
        items.push({ role: 'user', text: 'Call a tool to act on the computer, or call done or fail.', type: 'message' })
        continue
      }
      items.push({ type: 'function_call', ...call })
      if (call.name === 'done' || call.name === 'fail') {
        result.steps.push({ message: reply.text || undefined, tool: call.name })
        result.lastAction = call.name === 'done' ? 'DONE' : 'FAIL'
        return result
      }
      const output = await tools.call(call, options.signal)
      items.push({ callId: call.callId, images: output.images, text: output.text, type: 'function_call_output' })
      if (output.runId != null) {
        result.runIds.push(output.runId)
      }
      result.steps.push({
        arguments: call.arguments,
        message: reply.text || undefined,
        output: output.text.slice(0, 2_000),
        runId: output.runId,
        tool: call.name,
      })
    }
  }
  catch (error) {
    throw new AgentRunError(result, error)
  }
  return result
}

/** Keeps images only on the latest {@link imagesInHistory} tool outputs. */
function windowImages(items: ConversationItem[]): ConversationItem[] {
  let kept = 0
  return items.toReversed().map((item) => {
    if (item.type !== 'function_call_output' || item.images.length === 0) {
      return item
    }
    kept += 1
    return kept <= imagesInHistory ? item : { ...item, images: [], text: `${item.text}\n[Earlier screenshot omitted.]` }
  }).toReversed()
}
