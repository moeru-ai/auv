import type { AgentTools, FunctionCall, ModelEndpoint } from '../../agent'

import { afterEach, describe, expect, it, vi } from 'vitest'

import { AgentRunError } from '../../agent'
import { runAgent } from './runAgent'

const endpoint = (api: ModelEndpoint['api']): ModelEndpoint => ({ api, apiKey: 'k', baseURL: 'http://proxy.test/v1/', model: 'm' })

const invokeTool = { description: 'Invoke one AUV command.', name: 'invoke', parameters: { properties: { command_id: { type: 'string' } }, type: 'object' } }

/** Captures every model request and answers from `replies` in order. */
function stubModel(replies: unknown[]): Array<{ body: Record<string, any>, url: string }> {
  const requests: Array<{ body: Record<string, any>, url: string }> = []
  vi.stubGlobal('fetch', vi.fn(async (url: string, init: RequestInit) => {
    requests.push({ body: JSON.parse(String(init.body)), url })
    return Response.json(replies[requests.length - 1])
  }))
  return requests
}

/** Tool stub: every call returns text plus one image, and is recorded. */
function stubTools(): { calls: FunctionCall[], tools: AgentTools } {
  const calls: FunctionCall[] = []
  return {
    calls,
    tools: {
      call: async (call) => {
        calls.push(call)
        return { images: [`data:image/webp;base64,shot-${calls.length}`], runId: `run-${calls.length}`, text: `{"status":"completed","n":${calls.length}}` }
      },
      definitions: [invokeTool],
    },
  }
}

const call = (callId: string, name: string, args: unknown = {}) => ({ arguments: JSON.stringify(args), call_id: callId, name, type: 'function_call' })

afterEach(() => {
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

describe('runAgent with Responses tools', () => {
  it('executes the chosen AUV call and returns its output with the image', async () => {
    const requests = stubModel([
      // Reasoning items precede the call; only function_call items are actions.
      { output: [{ type: 'reasoning' }, call('c1', 'invoke', { command_id: 'display.capture' })] },
      { output: [call('c2', 'done')] },
    ])
    const { calls, tools } = stubTools()

    const result = await runAgent('do it', endpoint('responses'), tools, { maxSteps: 3, signal: AbortSignal.timeout(10_000) })

    expect(result.lastAction).toBe('DONE')
    expect(result.runIds).toEqual(['run-1'])
    expect(calls.map(item => item.arguments)).toEqual(['{"command_id":"display.capture"}'])
    const first = requests[0].body
    expect(first).toMatchObject({ model: 'm', parallel_tool_calls: false, store: false, tool_choice: 'auto' })
    expect(first.tools.map((tool: { name: string }) => tool.name)).toEqual(['invoke', 'done', 'fail'])
    expect(requests[1].body.input.slice(1)).toEqual([
      { arguments: '{"command_id":"display.capture"}', call_id: 'c1', name: 'invoke', type: 'function_call' },
      {
        call_id: 'c1',
        output: [
          { text: '{"status":"completed","n":1}', type: 'input_text' },
          { image_url: 'data:image/webp;base64,shot-1', type: 'input_image' },
        ],
        type: 'function_call_output',
      },
    ])
  })

  it('sums reply usage and reports every request to telemetry', async () => {
    stubModel([
      { output: [call('c1', 'invoke')], usage: { input_tokens: 100, input_tokens_details: { cached_tokens: 40 }, output_tokens: 10, total_tokens: 110 } },
      { output: [call('c2', 'done')], usage: { input_tokens: 150, input_tokens_details: { cached_tokens: 90 }, output_tokens: 5, total_tokens: 155 } },
    ])
    const { tools } = stubTools()
    const events: string[] = []
    const telemetry = {
      failed: () => events.push('failed'),
      requested: (step: number) => events.push(`requested ${step}`),
      responded: (reply: { usage?: Record<string, number> }) => events.push(`responded ${reply.usage?.total_tokens}`),
    }

    const result = await runAgent('do it', endpoint('responses'), tools, { maxSteps: 3, signal: AbortSignal.timeout(10_000), telemetry })

    expect(result.modelRequests).toBe(2)
    expect(result.usage).toEqual({ 'input_tokens': 250, 'input_tokens_details.cached_tokens': 130, 'output_tokens': 15, 'total_tokens': 265 })
    expect(events).toEqual(['requested 0', 'responded 110', 'requested 1', 'responded 155'])
  })

  it('ends with FAIL when the model calls fail', async () => {
    stubModel([{ output: [call('c1', 'fail')] }])
    const { calls, tools } = stubTools()

    const result = await runAgent('impossible', endpoint('responses'), tools, { maxSteps: 3, signal: AbortSignal.timeout(10_000) })

    expect(result.lastAction).toBe('FAIL')
    expect(calls).toEqual([])
  })

  it('keeps only the latest screenshot in history', async () => {
    const requests = stubModel([
      { output: [call('c1', 'invoke')] },
      { output: [call('c2', 'invoke')] },
      { output: [call('c3', 'done')] },
    ])
    const { tools } = stubTools()

    await runAgent('do it', endpoint('responses'), tools, { maxSteps: 5, signal: AbortSignal.timeout(10_000) })

    const outputs = requests[2].body.input.filter((item: { type?: string }) => item.type === 'function_call_output')
    expect(outputs[0].output).toBe('{"status":"completed","n":1}\n[Earlier screenshot omitted.]')
    expect(outputs[1].output[1]).toEqual({ image_url: 'data:image/webp;base64,shot-2', type: 'input_image' })
  })

  it('asks for a tool call when the model only answers in text', async () => {
    const requests = stubModel([
      { output: [{ content: [{ text: 'I will look first.', type: 'output_text' }], type: 'message' }] },
      { output: [call('c1', 'done')] },
    ])
    const { tools } = stubTools()

    const result = await runAgent('do it', endpoint('responses'), tools, { maxSteps: 3, signal: AbortSignal.timeout(10_000) })

    expect(result.steps[0]).toEqual({ message: 'I will look first.' })
    expect(requests[1].body.input.at(-1)).toEqual({ content: [{ text: 'Call a tool to act on the computer, or call done or fail.', type: 'input_text' }], role: 'user' })
  })

  // ROOT CAUSE:
  //
  // If a model request failed mid-episode (chrome/47543840: HTTP 400
  // `invalid_prompt` from moderation), runAgent threw and its earlier steps
  // were lost, so result.json could not show what the model had seen.
  //
  // The fix throws AgentRunError carrying the partial result.
  it('keeps earlier steps when a later model request fails', async () => {
    let calls = 0
    vi.stubGlobal('fetch', vi.fn(async () => {
      calls += 1
      return calls === 1
        ? Response.json({ output: [call('c1', 'invoke')] })
        : Response.json({ error: { code: 'invalid_prompt' } }, { status: 400 })
    }))
    const { tools } = stubTools()

    const failure = await runAgent('do it', endpoint('responses'), tools, { maxSteps: 3, signal: AbortSignal.timeout(10_000) }).catch((error: unknown) => error)

    expect(failure).toBeInstanceOf(AgentRunError)
    expect((failure as AgentRunError).message).toBe('Agent stopped after 1 steps')
    expect((failure as AgentRunError).partial.steps.map(step => step.tool)).toEqual(['invoke'])
    expect(String((failure as AgentRunError).cause)).toContain('HTTP 400')
  })
})
