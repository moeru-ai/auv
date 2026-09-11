import { defineRule } from '@alint-js/plugin'

import { judgeSource } from '../../agents/judge'
import { vacantControlBoundaryInstructions, vacantControlBoundaryPrompt } from './prompt'

export const vacantControlBoundaryRule = defineRule({
  create: ctx => ({
    /**
     * Reviews one Rust source target for vacant control boundaries.
     *
     * Triggering workflow:
     *
     * {@link vacantControlBoundaryRule}
     *   -> `onTargetFile`
     *     -> {@link judgeSource}
     *
     * Upstream:
     * - {@link vacantControlBoundaryRule}
     *
     * Downstream:
     * - {@link judgeSource}
     */
    async onTargetFile(target) {
      const findings = await judgeSource({
        context: ctx,
        instructions: vacantControlBoundaryInstructions,
        operation: 'vacant-control-boundary-review',
        prompt: `${vacantControlBoundaryPrompt}\n\nFile path:\n${target.file.path}`,
        source: ctx.src.getText(await ctx.src.readFile(target.file)),
      })

      for (const finding of findings) {
        ctx.report({
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
