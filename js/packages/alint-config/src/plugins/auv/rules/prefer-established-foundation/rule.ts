import { defineRule } from "@alint-js/plugin";

import { judgeSource } from "../../agents/judge";
import { establishedFoundationInstructions, establishedFoundationPrompt } from "./prompt";

export const establishedFoundationRule = defineRule({
  create: ctx => ({
    async onTargetFile(target) {
      const findings = await judgeSource({
        context: ctx,
        instructions: establishedFoundationInstructions,
        operation: "established-foundation-review",
        prompt: establishedFoundationPrompt,
        source: ctx.src.getText(target),
      });

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
        });
      }
    },
  }),
});
