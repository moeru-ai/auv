import type { AgentMessage } from '@earendil-works/pi-agent-core'
import type { Context } from '@earendil-works/pi-ai'

import type { AgentTools, FunctionCall, ModelEndpoint } from '../../agent'

import { createFauxCore, fauxAssistantMessage, fauxText, fauxToolCall } from '@earendil-works/pi-ai'
import { describe, expect, it } from 'vitest'

import { AgentRunError } from '../../agent'
import { observationPolicyFrom, runAgent, windowImages } from './runAgent'

const endpoint: ModelEndpoint = { api: 'responses', apiKey: 'k', baseURL: 'http://proxy.test/v1/', model: 'm' }

const invokeTool = { description: 'Invoke one AUV command.', name: 'invoke', parameters: { properties: { command_id: { type: 'string' }, inputs: { type: 'object' } }, type: 'object' } }

/** A faux model that answers `replies` in order and records every request context. */
function fauxModel(replies: Array<ReturnType<typeof fauxAssistantMessage>>) {
  const faux = createFauxCore({})
  const contexts: Context[] = []
  faux.setResponses(replies.map(reply => (context: Context) => {
    contexts.push(structuredClone(context))
    return reply
  }))
  return { contexts, model: { model: faux.getModel(), streamFn: faux.streamSimple } }
}

/** Tool stub: every call returns text plus one image, and is recorded. */
function stubTools(): { calls: FunctionCall[], tools: AgentTools } {
  const calls: FunctionCall[] = []
  return {
    calls,
    tools: {
      call: async (call) => {
        calls.push(call)
        return { images: [`data:image/webp;base64,shot${calls.length}`], runId: `run-${calls.length}`, text: `{"status":"completed","n":${calls.length}}` }
      },
      definitions: [invokeTool],
    },
  }
}

const invoke = (commandId: string) => fauxAssistantMessage(fauxToolCall('invoke', { command_id: commandId, inputs: {} }), { stopReason: 'toolUse' })
const end = (name: 'done' | 'fail') => fauxAssistantMessage(fauxToolCall(name, {}), { stopReason: 'toolUse' })

function imagesIn(context: Context): string[] {
  return context.messages.flatMap(message =>
    message.role === 'toolResult' ? message.content.flatMap(part => (part.type === 'image' ? [part.data] : [])) : [])
}

describe('runAgent on pi-agent-core', () => {
  it('executes the chosen AUV call, records it, and ends on done', async () => {
    const { calls, tools } = stubTools()
    const { contexts, model } = fauxModel([invoke('display.capture'), end('done')])

    const result = await runAgent('Open the file.', endpoint, tools, { maxSteps: 5, model, signal: AbortSignal.timeout(10_000) })

    expect(calls).toHaveLength(1)
    expect(JSON.parse(calls[0].arguments)).toEqual({ command_id: 'display.capture', inputs: {} })
    expect(result.lastAction).toBe('DONE')
    expect(result.runIds).toEqual(['run-1'])
    expect(result.steps.map(step => step.tool)).toEqual(['invoke', 'done'])
    expect(result.steps[0]).toMatchObject({ output: '{"status":"completed","n":1}', runId: 'run-1' })
    expect(result.modelRequests).toBe(2)
    expect(contexts[0].messages.find(message => message.role === 'user')).toMatchObject({ content: [{ text: 'Task: Open the file.', type: 'text' }] })
    expect(imagesIn(contexts[1])).toEqual(['shot1'])
  })

  it('observes the screen after an input action', async () => {
    const { calls, tools } = stubTools()
    const { contexts, model } = fauxModel([invoke('input.click'), end('done')])

    const result = await runAgent('Click.', endpoint, tools, { maxSteps: 5, model, signal: AbortSignal.timeout(10_000) })

    expect(calls.map(call => JSON.parse(call.arguments).command_id)).toEqual(['input.click', 'display.capture'])
    expect(imagesIn(contexts[1])).toEqual(['shot1', 'shot2'])
    expect(result.runIds).toEqual(['run-2', 'run-1'])
  })

  it('does not observe when the policy turns it off', async () => {
    const { calls, tools } = stubTools()
    const { model } = fauxModel([invoke('input.click'), end('done')])

    await runAgent('Click.', endpoint, tools, { maxSteps: 5, model, observation: { imagesInHistory: 3, observeAfterAction: false }, signal: AbortSignal.timeout(10_000) })

    expect(calls).toHaveLength(1)
  })

  it('ends with FAIL when the model calls fail', async () => {
    const { tools } = stubTools()
    const { model } = fauxModel([end('fail')])

    const result = await runAgent('Impossible.', endpoint, tools, { maxSteps: 5, model, signal: AbortSignal.timeout(10_000) })

    expect(result.lastAction).toBe('FAIL')
    expect(result.steps).toEqual([{ message: undefined, tool: 'fail' }])
  })

  it('asks for a tool call when the model only answers in text', async () => {
    const { tools } = stubTools()
    const { contexts, model } = fauxModel([fauxAssistantMessage(fauxText('I will look first.')), end('done')])

    const result = await runAgent('Look.', endpoint, tools, { maxSteps: 5, model, signal: AbortSignal.timeout(10_000) })

    expect(result.steps).toEqual([{ message: 'I will look first.' }, { message: undefined, tool: 'done' }])
    expect(contexts[1].messages.at(-1)).toMatchObject({ content: 'Call a tool to act on the computer, or call done or fail.', role: 'user' })
  })

  it('stops at the step budget without a terminal action', async () => {
    const { calls, tools } = stubTools()
    const { model } = fauxModel([invoke('display.capture'), invoke('display.capture'), invoke('display.capture')])

    const result = await runAgent('Loop.', endpoint, tools, { maxSteps: 2, model, signal: AbortSignal.timeout(10_000) })

    expect(calls).toHaveLength(2)
    expect(result.steps).toHaveLength(2)
    expect(result.lastAction).toBeUndefined()
  })

  it('keeps earlier steps when a later model request is rejected', async () => {
    const { tools } = stubTools()
    const { model } = fauxModel([invoke('display.capture'), fauxAssistantMessage('', { errorMessage: '400 invalid_request_error', stopReason: 'error' })])

    const failure = await runAgent('Fail later.', endpoint, tools, { maxSteps: 5, model, signal: AbortSignal.timeout(10_000) }).catch((error: unknown) => error)

    expect(failure).toBeInstanceOf(AgentRunError)
    expect((failure as AgentRunError).partial.steps.map(step => step.tool)).toEqual(['invoke'])
    expect(String((failure as AgentRunError).cause)).toContain('400 invalid_request_error')
  })
})

describe('windowImages', () => {
  const shot = (n: number): AgentMessage => ({
    content: [{ text: `out ${n}`, type: 'text' }, { data: `shot${n}`, mimeType: 'image/webp', type: 'image' }],
    isError: false,
    role: 'toolResult',
    timestamp: n,
    toolCallId: `c${n}`,
    toolName: 'invoke',
  })

  it('keeps images on the latest results and the text of older ones', () => {
    const windowed = windowImages([shot(1), shot(2), shot(3)], 2)

    expect(windowed.map(message => message.role === 'toolResult' && message.content.map(part => (part.type === 'image' ? part.data : part.text)))).toEqual([
      ['out 1', '[Earlier screenshot omitted.]'],
      ['out 2', 'shot2'],
      ['out 3', 'shot3'],
    ])
  })
})

describe('observationPolicyFrom', () => {
  it('defaults to observing after actions with three screenshots', () => {
    expect(observationPolicyFrom({})).toEqual({ imagesInHistory: 3, observeAfterAction: true })
    expect(observationPolicyFrom({ OSWORLD_IMAGES_IN_HISTORY: '1', OSWORLD_OBSERVE_AFTER_ACTION: 'false' })).toEqual({ imagesInHistory: 1, observeAfterAction: false })
  })

  it('rejects a non-positive screenshot window', () => {
    expect(() => observationPolicyFrom({ OSWORLD_IMAGES_IN_HISTORY: '0' })).toThrow('OSWORLD_IMAGES_IN_HISTORY')
  })
})
