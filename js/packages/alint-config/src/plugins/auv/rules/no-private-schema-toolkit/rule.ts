import { defineRule } from "@alint-js/plugin";

import { judgeSource } from "../../agents/judge";
import { privateSchemaToolkitInstructions, privateSchemaToolkitPrompt } from "./prompt";

export const privateSchemaToolkitRule = defineRule({
  create: ctx => ({
    async onTargetFile(target) {
      const findings = await judgeSource({
        context: ctx,
        instructions: privateSchemaToolkitInstructions,
        operation: "private-schema-toolkit-review",
        prompt: `${privateSchemaToolkitPrompt}\n\nFile path:\n${target.file.path}`,
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
