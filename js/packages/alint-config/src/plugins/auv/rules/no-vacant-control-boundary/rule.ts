import { defineRule } from "@alint-js/plugin";

import { judgeSource } from "../../agents/judge";
import { vacantControlBoundaryInstructions, vacantControlBoundaryPrompt } from "./prompt";

export const vacantControlBoundaryRule = defineRule({
  create: ctx => ({
    async onTargetFile(target) {
      const findings = await judgeSource({
        context: ctx,
        instructions: vacantControlBoundaryInstructions,
        operation: "vacant-control-boundary-review",
        prompt: vacantControlBoundaryPrompt,
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
