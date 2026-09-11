import type { FileTarget, RuleContext } from '@alint-js/plugin'

import { defineRule } from '@alint-js/plugin'

import { judgeSource } from '../../agents/judge'
import { nonRuntimeUnitTestsInstructions, nonRuntimeUnitTestsPrompt } from './prompt'

export const nonRuntimeUnitTestsRule = defineRule({
  create: context => ({
    /**
     * Reviews tests in a non-runtime crate for an earned local responsibility.
     *
     * Triggering workflow:
     *
     * {@link nonRuntimeUnitTestsRule}
     *   -> `onTargetFile`
     *     -> {@link reviewNonRuntimeUnitTests}
     *
     * Upstream:
     * - {@link nonRuntimeUnitTestsRule}
     *
     * Downstream:
     * - {@link judgeSource}
     */
    onTargetFile: target => reviewNonRuntimeUnitTests(context, target),
  }),
})

async function reviewNonRuntimeUnitTests(context: RuleContext, target: FileTarget): Promise<void> {
  const source = context.src.getText(await context.src.readFile(target.file))
  const isTestPath = target.file.path.includes('/tests/') || target.file.path.endsWith('_test.rs')
  if (!isTestPath && !source.includes('cfg(test)') && !source.includes('#[test]')) {
    return
  }

  const findings = await judgeSource({
    context,
    instructions: nonRuntimeUnitTestsInstructions,
    operation: 'non-runtime-unit-tests-review',
    prompt: `${nonRuntimeUnitTestsPrompt}\n\nFile path:\n${target.file.path}`,
    source,
  })

  for (const finding of findings) {
    context.report({
      evidence: {
        confidence: finding.confidence,
        suggestion: finding.suggestion,
      },
      filePath: target.file.path,
      loc: { start: { column: 0, line: finding.line } },
      message: finding.message,
    })
  }
}
