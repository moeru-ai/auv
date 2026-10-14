import type { ModelEndpoint } from './agent'

import { afterEach, describe, expect, it, vi } from 'vitest'

import { complete, modelApiFrom, requestFor, usageOf } from './agent'

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

afterEach(() => {
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

describe('requestFor', () => {
  // ROOT CAUSE:
  //
  // If a request declared no tools and left tool choice open, the Codex
  // backend ran its injected image_generation tool between answers, several
  // rounds per request (44ee5668 step 1 replayed: 29-173 s; with tool_choice
  // none: 4 s, one message).
  it('disables tools for a text-only Responses request', () => {
    const request = requestFor(endpoint('responses'), { items: [{ role: 'user', text: 'Reply with OK.', type: 'message' }] })
    expect(JSON.parse(request.body)).toMatchObject({ store: false, tool_choice: 'none' })
  })

  it('maps tool calls onto Chat Completions, with images in a following user turn', () => {
    const request = requestFor(endpoint('chat'), {
      instructions: 'system',
      items: [
        { role: 'user', text: 'Task: do it', type: 'message' },
        { arguments: '{}', callId: 'c1', name: 'invoke', type: 'function_call' },
        { callId: 'c1', images: ['data:image/webp;base64,x'], text: 'ok', type: 'function_call_output' },
      ],
      tools: [invokeTool],
    })

    expect(request.url).toBe('http://proxy.test/v1/chat/completions')
    const body = JSON.parse(request.body)
    expect(body.tools).toEqual([{ function: invokeTool, type: 'function' }])
    expect(body.messages).toEqual([
      { content: 'system', role: 'system' },
      { content: 'Task: do it', role: 'user' },
      { content: null, role: 'assistant', tool_calls: [{ function: { arguments: '{}', name: 'invoke' }, id: 'c1', type: 'function' }] },
      { content: 'ok', role: 'tool', tool_call_id: 'c1' },
      { content: [{ text: 'Images from c1:', type: 'text' }, { image_url: { url: 'data:image/webp;base64,x' }, type: 'image_url' }], role: 'user' },
    ])
  })
})

describe('complete', () => {
  // ROOT CAUSE:
  //
  // If cliproxyapi loses its upstream stream, it answers HTTP 408
  // ("stream closed before response.completed"). Only 429 and 5xx were
  // retried, so one dropped stream failed the whole OSWorld episode
  // (chrome/3720f614 in the 2026-10-11 test_nogdrive run).
  //
  // The fix retries 408 like the other transient proxy failures.
  it('retries a 408 request timeout from the proxy', async () => {
    vi.useFakeTimers()
    let calls = 0
    vi.stubGlobal('fetch', vi.fn(async () => {
      calls += 1
      return calls === 1
        ? Response.json({ error: { message: 'stream closed before response.completed' } }, { status: 408 })
        : Response.json({ output: [{ content: [{ text: 'ok', type: 'output_text' }], type: 'message' }] })
    }))
    const reply = complete(endpoint('responses'), { items: [{ role: 'user', text: 'hi', type: 'message' }] }, new AbortController().signal)
    await vi.runAllTimersAsync()
    await expect(reply).resolves.toEqual({ calls: [], text: 'ok' })
    expect(calls).toBe(2)
  })

  // ROOT CAUSE:
  //
  // If every Codex account behind cliproxyapi cooled down, the proxy answered
  // 503 `auth_unavailable` for minutes. Three attempts 15 s apart gave up, so
  // 14 of 22 episodes in the 2026-10-12 run errored mid-trajectory.
  //
  // The fix waits out 429/503 with a longer, bounded backoff.
  it('waits out a proxy with no available account', async () => {
    vi.useFakeTimers()
    let calls = 0
    vi.stubGlobal('fetch', vi.fn(async () => {
      calls += 1
      return calls <= 5
        ? Response.json({ error: { message: 'auth_unavailable: no auth available (providers=codex, model=gpt-5.5)' } }, { status: 503 })
        : Response.json({ output: [{ content: [{ text: 'ok', type: 'output_text' }], type: 'message' }] })
    }))
    const reply = complete(endpoint('responses'), { items: [] }, new AbortController().signal)
    await vi.runAllTimersAsync()
    await expect(reply).resolves.toEqual({ calls: [], text: 'ok' })
    expect(calls).toBe(6)
  })

  // ROOT CAUSE:
  //
  // If the upstream model was overloaded, cliproxyapi relayed 502
  // `service_unavailable_error`. 502 counted as a short transient error, so
  // three quick retries gave up and 4 of 55 gpt-6.1-sol episodes errored.
  //
  // The fix waits out 502 like 503.
  it('waits out an overloaded upstream model', async () => {
    vi.useFakeTimers()
    let calls = 0
    vi.stubGlobal('fetch', vi.fn(async () => {
      calls += 1
      return calls <= 5
        ? Response.json({ error: { code: 'server_is_overloaded', type: 'service_unavailable_error' } }, { status: 502 })
        : Response.json({ output: [{ content: [{ text: 'ok', type: 'output_text' }], type: 'message' }] })
    }))
    const reply = complete(endpoint('responses'), { items: [] }, new AbortController().signal)
    await vi.runAllTimersAsync()
    await expect(reply).resolves.toEqual({ calls: [], text: 'ok' })
    expect(calls).toBe(6)
  })

  it('honors Retry-After from a rate-limited proxy', async () => {
    vi.useFakeTimers()
    let calls = 0
    vi.stubGlobal('fetch', vi.fn(async () => {
      calls += 1
      return calls === 1
        ? new Response('slow down', { headers: { 'retry-after': '90' }, status: 429 })
        : Response.json({ output: [] })
    }))
    const reply = complete(endpoint('responses'), { items: [] }, new AbortController().signal)
    await vi.advanceTimersByTimeAsync(89_000)
    expect(calls).toBe(1)
    await vi.advanceTimersByTimeAsync(2_000)
    await expect(reply).resolves.toEqual({ calls: [], text: '' })
  })

  it('still fails fast on a request the proxy rejects', async () => {
    let calls = 0
    vi.stubGlobal('fetch', vi.fn(async () => {
      calls += 1
      return Response.json({ error: { code: 'invalid_prompt' } }, { status: 400 })
    }))
    await expect(complete(endpoint('responses'), { items: [] }, new AbortController().signal)).rejects.toThrow('HTTP 400')
    expect(calls).toBe(1)
  })

  it('reads Chat Completions tool calls', async () => {
    stubModel([{ choices: [{ message: { content: null, tool_calls: [{ function: { arguments: '{"a":1}', name: 'invoke' }, id: 'c9' }] } }] }])
    const reply = await complete(endpoint('chat'), { items: [], tools: [invokeTool] }, AbortSignal.timeout(10_000))
    expect(reply).toEqual({ calls: [{ arguments: '{"a":1}', callId: 'c9', name: 'invoke' }], text: '' })
  })
})

describe('modelApiFrom', () => {
  it('defaults to the Responses API', () => {
    expect(modelApiFrom(undefined)).toBe('responses')
    expect(modelApiFrom('')).toBe('responses')
    expect(modelApiFrom('chat')).toBe('chat')
  })

  it('rejects an unknown API instead of silently falling back', () => {
    expect(() => modelApiFrom('completions')).toThrow(/responses" or "chat/)
  })
})

describe('usageOf', () => {
  it('flattens Chat Completions usage and ignores a reply without usage', () => {
    expect(usageOf({ usage: { completion_tokens: 3, completion_tokens_details: { reasoning_tokens: 2 }, prompt_tokens: 9, total_tokens: 12 } }))
      .toEqual({ 'completion_tokens': 3, 'completion_tokens_details.reasoning_tokens': 2, 'prompt_tokens': 9, 'total_tokens': 12 })
    expect(usageOf({ output: [] })).toBeUndefined()
  })
})
