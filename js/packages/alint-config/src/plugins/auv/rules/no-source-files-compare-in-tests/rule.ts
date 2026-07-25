import type { FileTarget, RuleContext } from "@alint-js/plugin";

import { defineRule } from "@alint-js/plugin";

import { judgeSource } from "../../agents/judge";
import { noSourceFilesCompareInTestsInstructions, noSourceFilesCompareInTestsPrompt } from "./prompt";

export const noSourceFilesCompareInTestsRule = defineRule({
  create: context => ({
    /**
     * Reviews one Rust source target for tests that inspect source-file layout.
     *
     * Triggering workflow:
     *
     * {@link noSourceFilesCompareInTestsRule}
     *   -> `onTargetFile`
     *     -> {@link reviewSourceFileComparisons}
     *
     * Upstream:
     * - {@link noSourceFilesCompareInTestsRule}
     *
     * Downstream:
     * - {@link judgeSource}
     */
    onTargetFile: target => reviewSourceFileComparisons(context, target),
  }),
});

async function reviewSourceFileComparisons(context: RuleContext, target: FileTarget): Promise<void> {
  const findings = await judgeSource({
    context,
    instructions: noSourceFilesCompareInTestsInstructions,
    operation: "source-files-compare-in-tests-review",
    prompt: `${noSourceFilesCompareInTestsPrompt}\n\nFile path:\n${target.file.path}`,
    source: context.src.getText(target),
  });

  for (const finding of findings) {
    context.report({
      evidence: {
        confidence: finding.confidence,
        suggestion: finding.suggestion,
      },
      filePath: target.file.path,
      loc: { start: { column: 0, line: finding.line } },
      message: finding.message,
    });
  }
}
