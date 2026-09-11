import { resolve } from 'node:path'

import { describe, expect, it, vi } from 'vitest'

import auvPlugin from '../../index'

import { platformScopedAppIntegrationPrompt } from './prompt'
import { platformScopedAppIntegrationRule } from './rule'

describe('require-platform-scoped-app-integration', () => {
  it('states the app integration conventions as semantic responsibilities', () => {
    expect(platformScopedAppIntegrationPrompt).toContain('src/platforms/')
    expect(platformScopedAppIntegrationPrompt).toContain('<capability>_<platform>.rs')
    expect(platformScopedAppIntegrationPrompt).toContain('Platform-specific view parser')
    expect(platformScopedAppIntegrationPrompt).toContain('one platform workflow cohesive')
  })

  it('is registered by the AUV plugin', () => {
    expect(auvPlugin.rules?.['require-platform-scoped-app-integration']).toBe(platformScopedAppIntegrationRule)
  })

  // https://github.com/moeru-ai/auv/actions/runs/31741822367/job/94586817052
  // ROOT CAUSE:
  //
  // If this test ran on Windows, the rule reported a native absolute path but
  // the assertion expected a hard-coded POSIX path.
  //
  // Before the fix, the valid finding failed only on Windows. The fix derives
  // the expectation with the same platform path semantics as the public report.
  it('reports a valid directory-level finding', async () => {
    const report = vi.fn()
    const recordUsage = vi.fn()
    const cratePath = '/repo/crates/auv-example-app'
    const context = createContext({
      agent: async (request: { tools: Array<{ execute: (input: unknown) => unknown, name: string }> }) => {
        const submit = request.tools.find(tool => tool.name === 'submit_platform_review')
        expect(submit).toBeDefined()
        await submit!.execute({
          findings: [
            {
              category: 'automation-placement',
              confidence: 'high',
              filePath: 'src/commands/search.rs',
              line: 3,
              message: 'Search automation is owned by a command module.',
              suggestion: 'Move the cohesive Windows search workflow to src/platforms/search_windows.rs.',
            },
          ],
        })
        return { answer: '', usage: { inputTokens: 10, outputTokens: 4, totalTokens: 14 } }
      },
      recordUsage,
      report,
    })

    await runDirectoryRule(context, cratePath)

    expect(report).toHaveBeenCalledWith(expect.objectContaining({
      evidence: expect.objectContaining({ category: 'automation-placement', confidence: 'high' }),
      filePath: resolve(cratePath, 'src/commands/search.rs'),
      loc: { start: { column: 0, line: 3 } },
    }))
    expect(recordUsage).toHaveBeenCalledWith(expect.objectContaining({ totalTokens: 14 }))
  })

  it('drops findings whose reported line is outside the source file', async () => {
    const report = vi.fn()
    const context = createContext({
      agent: async (request: { tools: Array<{ execute: (input: unknown) => unknown, name: string }> }) => {
        const submit = request.tools.find(tool => tool.name === 'submit_platform_review')!
        await submit.execute({
          findings: [
            {
              category: 'platform-file',
              confidence: 'medium',
              filePath: 'src/search.rs',
              line: 99,
              message: 'The platform owner is unclear.',
              suggestion: 'Use a capability and platform file owner.',
            },
          ],
        })
        return { answer: '' }
      },
      report,
    })

    await runDirectoryRule(context, '/repo/crates/auv-example-app')

    expect(report).not.toHaveBeenCalled()
  })
})

function createContext(options: {
  agent: (request: any) => Promise<any>
  recordUsage?: ReturnType<typeof vi.fn>
  report: ReturnType<typeof vi.fn>
}) {
  return {
    agent: options.agent,
    cwd: '/repo',
    id: 'rust/require-platform-scoped-app-integration',
    localId: 'require-platform-scoped-app-integration',
    logger: { debug: vi.fn() },
    metering: { recordUsage: options.recordUsage ?? vi.fn() },
    model: async () => ({ id: 'test-model', provider: { endpoint: 'http://unused', headers: {}, id: 'test' } }),
    options: [],
    report: options.report,
    settings: {},
    src: {
      getText: vi.fn(),
      readFile: async (filePath: string) => ({
        language: 'text/plain',
        lines: ['one', 'two', 'three', 'four'],
        path: filePath,
        text: 'one\ntwo\nthree\nfour\n',
      }),
    },
  }
}

async function runDirectoryRule(context: ReturnType<typeof createContext>, path: string): Promise<void> {
  const handlers = platformScopedAppIntegrationRule.create(context as never)
  if (!('onTargetDirectory' in handlers) || !handlers.onTargetDirectory) {
    throw new Error('directory handler missing')
  }
  await handlers.onTargetDirectory({ kind: 'directory', path })
}
