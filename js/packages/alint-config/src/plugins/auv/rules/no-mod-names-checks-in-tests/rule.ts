import type { FileTarget, RuleContext } from '@alint-js/plugin'

import { defineRule } from '@alint-js/plugin'

import { judgeSource } from '../../agents/judge'
import { noModNamesChecksInTestsInstructions, noModNamesChecksInTestsPrompt } from './prompt'

export const noModNamesChecksInTestsRule = defineRule({
  create: context => ({
    /**
     * Reviews one Rust source target for declaration-name assertions in tests.
     *
     * Triggering workflow:
     *
     * {@link noModNamesChecksInTestsRule}
     *   -> `onTargetFile`
     *     -> {@link reviewModuleNameChecks}
     *
     * Upstream:
     * - {@link noModNamesChecksInTestsRule}
     *
     * Downstream:
     * - {@link judgeSource}
     */
    onTargetFile: target => reviewModuleNameChecks(context, target),
  }),
})

async function reviewModuleNameChecks(context: RuleContext, target: FileTarget): Promise<void> {
  const findings = await judgeSource({
    context,
    instructions: noModNamesChecksInTestsInstructions,
    operation: 'mod-names-checks-in-tests-review',
    prompt: `${noModNamesChecksInTestsPrompt}\n\nFile path:\n${target.file.path}`,
    source: context.src.getText(await context.src.readFile(target.file)),
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
