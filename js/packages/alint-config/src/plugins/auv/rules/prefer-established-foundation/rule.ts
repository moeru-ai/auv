import { defineRule } from '@alint-js/plugin'

import { judgeSource } from '../../agents/judge'
import { establishedFoundationInstructions, establishedFoundationPrompt } from './prompt'

export const establishedFoundationRule = defineRule({
  create: ctx => ({
    /**
     * Reviews one Rust source target for private replacements of established foundations.
     *
     * Triggering workflow:
     *
     * {@link establishedFoundationRule}
     *   -> `onTargetFile`
     *     -> {@link judgeSource}
     *
     * Upstream:
     * - {@link establishedFoundationRule}
     *
     * Downstream:
     * - {@link judgeSource}
     */
    async onTargetFile(target) {
      const findings = await judgeSource({
        context: ctx,
        instructions: establishedFoundationInstructions,
        operation: 'established-foundation-review',
        prompt: `${establishedFoundationPrompt}\n\nFile path:\n${target.file.path}`,
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
