import { defineRule } from '@alint-js/plugin'

import { judgeSource } from '../../agents/judge'
import { unearnedFunctionBoundaryInstructions, unearnedFunctionBoundaryPrompt } from './prompt'

export const unearnedFunctionBoundaryRule = defineRule({
  cacheKey: `${unearnedFunctionBoundaryInstructions}\n${unearnedFunctionBoundaryPrompt}`,
  create: context => ({
    /**
     * Reviews one Rust source target for unearned function boundaries.
     *
     * Triggering workflow:
     *
     * {@link unearnedFunctionBoundaryRule}
     *   -> `onTargetFile`
     *     -> {@link judgeSource}
     *
     * Upstream:
     * - {@link unearnedFunctionBoundaryRule}
     *
     * Downstream:
     * - {@link judgeSource}
     */
    async onTargetFile(target) {
      const findings = await judgeSource({
        context,
        instructions: unearnedFunctionBoundaryInstructions,
        operation: 'unearned-function-boundary-review',
        prompt: `${unearnedFunctionBoundaryPrompt}\n\nFile path:\n${target.file.path}`,
        source: context.src.getText(await context.src.readFile(target.file)),
      })

      for (const finding of findings) {
        context.report({
          evidence: {
            confidence: finding.confidence,
            suggestion: finding.suggestion,
          },
          filePath: target.file.path,
          loc: {
            start: {
              column: 0,
              line: finding.line,
            },
          },
          message: finding.message,
        })
      }
    },
  }),
})
