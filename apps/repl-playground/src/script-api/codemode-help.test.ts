import catalog from 'virtual:auv-codemode-help'

import { describe, expect, it } from 'vitest'

import { compileCell } from '../stepper/compile'
import { runCodeMode } from '../workers/codemode'
import { createApiHelp } from './codemode-help'
import { codeModeSdk } from './codemode-sdk'

const api = createApiHelp(catalog)

describe('agent SDK help', () => {
  it('finds exact SDK paths and pages a namespace without hidden streaming methods', () => {
    const first = api.search({ limit: 2, namespace: 'WindowClient' })
    expect(first.items).toHaveLength(2)
    expect(first.remaining).toBeGreaterThan(0)
    const second = api.search({ limit: 2, namespace: 'WindowClient', offset: first.next!.offset })
    expect(second.items.map(item => item.path)).not.toContain(first.items[0].path)
    const found = api.search({ query: 'WindowClient.findText' })
    expect(found.items[0]).toMatchObject({ path: 'WindowClient.findText', signature: expect.stringContaining('query: string') })
    expect(api.search({ query: 'scrollStream' }).items.map(item => item.path)).not.toContain('WindowClient.scrollStream')
    expect(api.describe({ path: 'WindowClient.scrollStream' })).toMatchObject({ found: false })
  })

  it('describes SDK arguments and output fields rather than raw RPC call arguments', () => {
    const entry = api.describe({ path: 'WindowClient.findText' })
    expect(entry).toMatchObject({ examples: [expect.stringContaining('win.findText(')], found: true, signature: expect.stringContaining('Promise<FindWindowTextResponse>') })
    expect(JSON.stringify(entry)).toContain('matches')
    expect(JSON.stringify(entry)).not.toContain('AbortSignal')
    expect(catalog.types.FindWindowTextOptions.definition).not.toContain('window:')
    expect(catalog.types.FindWindowTextResponse.definition).toContain('capture?')
    const target = api.describe({ path: 'PointTarget' })
    expect(target).toMatchObject({ description: expect.stringContaining('window-local'), found: true })
    expect(api.describe({ path: 'not.real' })).toMatchObject({ found: false, message: expect.stringContaining('api.search') })
    expect(api.describe({ path: '__proto__' })).toMatchObject({ found: false })
  })

  it('exposes admitted SDK constants and excludes connection APIs', () => {
    expect(api.search({ namespace: 'sdk.Position' }).items.map(item => item.path)).toEqual(['sdk.Position.display', 'sdk.Position.screen', 'sdk.Position.window'])
    expect(api.describe({ path: 'sdk.InputPolicy' })).toMatchObject({ found: true })
    // Selected SDK exports keep their function JSDoc.
    expect(api.describe({ path: 'sdk.region' })).toMatchObject({ description: expect.stringMatching(/\S/) })
    expect(JSON.stringify(api.describe({ path: 'sdk.InputPolicy' }))).toContain('BACKGROUND_ONLY')
    expect(api.search({ query: 'connect' }).items.map(item => item.path)).not.toContain('sdk.connect')
    const helperNames = catalog.entries.filter(entry => entry.path.startsWith('sdk.')).map(entry => entry.path.split('.')[1])
    expect([...new Set(helperNames)].sort()).toEqual(Object.keys(codeModeSdk).sort())
    expect(() => api.search({ limit: 11 })).toThrow('limit from 1 to 10')
    expect(() => api.search({ offset: -1 })).toThrow('nonnegative offset')
  })

  // SDK docs own alias expansion. The VM catalog removes what cannot cross the VM.
  it('omits callback and AbortSignal fields from VM type definitions', () => {
    const definitions = Object.values(catalog.types).map(type => type.definition).join('\n')
    expect(catalog.types.ScrollUntilOptions.definition).toContain('maxSteps?: undefined | number')
    expect(definitions).not.toMatch(/\b(?:onUpdate|signal|until)\??:/)
    expect(definitions).not.toMatch(/\bAbortSignal\b/)
  })

  // The omitted-method list is data. This catches a new stream or generator method that it misses.
  it('advertises no stream or generator type that cannot cross the VM', () => {
    const text = JSON.stringify([...catalog.entries.map(entry => entry.signature), ...Object.values(catalog.types).map(type => type.definition)])
    expect(text).not.toMatch(/\b(?:AsyncIterable|AsyncGenerator|TypedDuplexCall|ScrollStreamController)\b/)
  })

  // ROOT CAUSE: enum member types were printed as imports with no numeric
  // values, and the agent VM did not expose the scroll completion enum.
  it('explains numeric scroll reasons and makes the enum usable in the VM', async () => {
    expect(api.describe({ path: 'ScrollUntilStopReason' })).toMatchObject({
      definition: expect.stringContaining('BUDGET_EXHAUSTED = 3'),
      found: true,
    })
    const outcome = await runCodeMode({ ...compileCell('text(sdk.ScrollUntilStopReason[3]);', { allowReturn: true }), breakpoints: [], codemode: { store: {}, timeoutMs: 2000 }, executionId: 1, mode: 'run' }, {
      __bind: () => {},
      __step: async () => {},
      __stepSync: () => {},
      auv: {},
      sdk: codeModeSdk,
    }, new AbortController().signal)
    expect(outcome.status).toBe('ok')
    expect(outcome.codemode!.content).toEqual([{ text: 'BUDGET_EXHAUSTED', type: 'text' }])
  })

  it('keeps each help result within the VM output budget and resolves its type links', () => {
    for (const path of [...catalog.entries.map(entry => entry.path), ...Object.keys(catalog.types)]) {
      const help = api.describe({ path })
      expect(help.found, path).toBe(true)
      expect(JSON.stringify(help).length, path).toBeLessThanOrEqual(16000)
      if ('relatedTypes' in help) {
        for (const name of help.relatedTypes!)
          expect(catalog.types[name], `${path} links to ${name}`).toBeDefined()
      }
    }
  })

  // Help examples must execute against the same SDK interface that they describe.
  it('shows how to output WindowClient data while keeping local callbacks inside the VM', async () => {
    const help = api.describe({ path: 'text' })
    expect(help).toMatchObject({ description: expect.stringContaining('functions') })
    // The `text` global owns the output example; SDK examples do not call VM globals.
    if (!('examples' in help))
      throw new Error('Missing text example')
    const example = help.examples[1]
    expect(api.help().rules.join(' ')).toContain('Array.map')
    const outcome = await runCodeMode({ ...compileCell(example, { allowReturn: true }), breakpoints: [], codemode: { store: {}, timeoutMs: 2000 }, executionId: 1, mode: 'run' }, {
      __bind: () => {},
      __step: async () => {},
      __stepSync: () => {},
      auv: { windows: { list: async () => [{ capture: async () => ({}), id: 'w-counter', window: { title: 'Counter' } }] } },
    }, new AbortController().signal)
    expect(outcome).toMatchObject({ codemode: { content: [{ text: '[{"id":"w-counter","window":{"title":"Counter"}}]', type: 'text' }] }, status: 'ok' })
  })
})
