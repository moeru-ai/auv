import type { RuleContext } from '@alint-js/plugin'

import { afterEach, describe, expect, it, vi } from 'vitest'

import { judgeSource } from './agent'

describe('judgeSource', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('forwards the rule cancellation signal to the model request', async () => {
    const controller = new AbortController()
    const fetchMock = vi.fn(async (_input: Parameters<typeof fetch>[0], init?: Parameters<typeof fetch>[1]) => {
      expect(init?.signal).toBe(controller.signal)
      return toolResponse({ findings: [] })
    })
    vi.stubGlobal('fetch', fetchMock)

    await judgeSource({
      context: createContext({ signal: controller.signal }),
      instructions: 'Review the source.',
      operation: 'test-review',
      prompt: 'Report findings.',
      source: 'fn main() {}',
    })

    expect(fetchMock).toHaveBeenCalledOnce()
  })

  it('feeds malformed tool arguments back to the model before retrying', async () => {
    const recordUsage = vi.fn()
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(malformedToolResponse('{"findings": ['))
      .mockResolvedValueOnce(toolResponse({ findings: [] }))
    vi.stubGlobal('fetch', fetchMock)

    const findings = await judgeSource({
      context: createContext({ metering: { recordUsage } }),
      instructions: 'Review the source.',
      operation: 'test-review',
      prompt: 'Report findings.',
      source: 'fn main() {}',
    })

    expect(findings).toEqual([])
    expect(fetchMock).toHaveBeenCalledTimes(2)
    const retryBody = fetchMock.mock.calls[1]?.[1]?.body
    expect(typeof retryBody).toBe('string')
    if (typeof retryBody !== 'string') {
      throw new TypeError('retry request body must be a string')
    }
    const retryMessages = JSON.parse(retryBody).messages
    expect(retryMessages.map((message: { role: string }) => message.role)).toEqual([
      'system',
      'user',
      'assistant',
      'tool',
      'user',
    ])
    expect(retryMessages[3]).toMatchObject({
      role: 'tool',
      tool_call_id: 'call_test',
    })
    expect(retryMessages[3].content).toContain('valid JSON')
    expect(retryMessages[4].content).toContain('report_findings')
    expect(recordUsage).toHaveBeenCalledTimes(2)
  })

  it('retries an invalid response body with JSON feedback', async () => {
    const recordUsage = vi.fn()
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(new Response('{"choices": ['))
      .mockResolvedValueOnce(toolResponse({ findings: [] }))
    vi.stubGlobal('fetch', fetchMock)

    await judgeSource({
      context: createContext({ metering: { recordUsage } }),
      instructions: 'Review the source.',
      operation: 'test-review',
      prompt: 'Report findings.',
      source: 'fn main() {}',
    })

    expect(fetchMock).toHaveBeenCalledTimes(2)
    const retryBody = fetchMock.mock.calls[1]?.[1]?.body
    expect(typeof retryBody).toBe('string')
    if (typeof retryBody !== 'string') {
      throw new TypeError('retry request body must be a string')
    }
    const retryMessages = JSON.parse(retryBody).messages
    expect(retryMessages.map((message: { role: string }) => message.role)).toEqual([
      'system',
      'user',
      'user',
    ])
    expect(retryMessages[2].content).toContain('not valid JSON')
    expect(recordUsage).toHaveBeenCalledOnce()
  })

  it('bounds malformed tool retries and records every response usage', async () => {
    const recordUsage = vi.fn()
    const fetchMock = vi.fn().mockImplementation(async () => malformedToolResponse('{"findings": ['))
    vi.stubGlobal('fetch', fetchMock)

    await expect(judgeSource({
      context: createContext({ metering: { recordUsage } }),
      instructions: 'Review the source.',
      operation: 'test-review',
      prompt: 'Report findings.',
      source: 'fn main() {}',
    })).rejects.toThrow('Unexpected end of JSON input')

    expect(fetchMock).toHaveBeenCalledTimes(3)
    expect(recordUsage).toHaveBeenCalledTimes(3)
  })
})

function createContext(overrides: Partial<RuleContext> = {}): RuleContext {
  return {
    cwd: '/repo',
    id: 'rust/test-rule',
    localId: 'test-rule',
    logger: { debug: vi.fn() },
    metering: { recordUsage: vi.fn() },
    model: async () => ({
      aliases: [],
      capabilities: ['tool-call'],
      id: 'test-model',
      name: 'test-model',
      params: {},
      provider: {
        endpoint: 'https://provider.example/v1',
        headers: { authorization: 'Bearer test' },
        id: 'test-provider',
        type: 'openai-compatible',
      },
    }),
    options: [],
    report: vi.fn(),
    settings: {},
    src: {} as RuleContext['src'],
    ...overrides,
  }
}

function malformedToolResponse(argumentsValue: string): Response {
  return toolArgumentsResponse(argumentsValue)
}

function toolArgumentsResponse(argumentsValue: string): Response {
  return Response.json({
    choices: [
      {
        finish_reason: 'tool_calls',
        message: {
          role: 'assistant',
          tool_calls: [
            {
              function: {
                arguments: argumentsValue,
                name: 'report_findings',
              },
              id: 'call_test',
              type: 'function',
            },
          ],
        },
      },
    ],
    usage: { completion_tokens: 3, prompt_tokens: 7, total_tokens: 10 },
  })
}

function toolResponse(argumentsValue: unknown): Response {
  return toolArgumentsResponse(JSON.stringify(argumentsValue))
}
