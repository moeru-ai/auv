import { defineRule } from '@alint-js/plugin'

import { judgeSource } from '../../agents/judge'
import { privateSchemaToolkitInstructions, privateSchemaToolkitPrompt } from './prompt'

export const privateSchemaToolkitRule = defineRule({
  create: ctx => ({
    /**
     * Reviews one Rust source target for private schema toolkits.
     *
     * Triggering workflow:
     *
     * {@link privateSchemaToolkitRule}
     *   -> `onTargetFile`
     *     -> {@link judgeSource}
     *
     * Upstream:
     * - {@link privateSchemaToolkitRule}
     *
     * Downstream:
     * - {@link judgeSource}
     */
    async onTargetFile(target) {
      const findings = await judgeSource({
        context: ctx,
        instructions: privateSchemaToolkitInstructions,
        operation: 'private-schema-toolkit-review',
        prompt: `${privateSchemaToolkitPrompt}\n\nFile path:\n${target.file.path}`,
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
