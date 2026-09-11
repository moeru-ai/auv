import type { FileTarget, RuleContext } from '@alint-js/plugin'

import { readFile } from 'node:fs/promises'
import { basename, dirname, join } from 'node:path'

import { defineRule } from '@alint-js/plugin'

import { judgeSource } from '../../agents/judge'
import { sideBySideUnitTestsInstructions, sideBySideUnitTestsPrompt } from './prompt'

export const sideBySideUnitTestsRule = defineRule({
  create: context => ({
    /**
     * Reviews a test-bearing production file for AUV's side-by-side convention.
     *
     * Triggering workflow:
     *
     * {@link sideBySideUnitTestsRule}
     *   -> `onTargetFile`
     *     -> {@link reviewSideBySideUnitTests}
     *
     * Upstream:
     * - {@link sideBySideUnitTestsRule}
     *
     * Downstream:
     * - {@link judgeSource}
     */
    onTargetFile: target => reviewSideBySideUnitTests(context, target),
  }),
})

/**
 * Validates a production file whose only test code is exact sidecar wiring.
 *
 * Triggering workflow:
 *
 * {@link sideBySideUnitTestsRule}
 *   -> {@link reviewSideBySideUnitTests}
 *     -> exact owner wiring
 *       -> {@link reviewCompleteOwnerWiring}
 *
 * Upstream:
 * - {@link reviewSideBySideUnitTests}
 *
 * Downstream:
 * - {@link readFile}
 * - {@link RuleContext.report}
 */
async function reviewCompleteOwnerWiring(context: RuleContext, target: FileTarget, source: string): Promise<boolean> {
  if (source.includes('#[test]') || source.includes('#[tokio::test]')) {
    return false
  }

  const wiring = [
    ...source.matchAll(/#\[cfg\((?:test|all\(\s*test\b[^\]]*)\)\]\s*#\[path\s*=\s*"([^"]+)"\]\s*mod\s+tests\s*;/g),
  ]
  const testDataWiring = [
    ...source.matchAll(
      /#\[cfg\(test\)\]\s*(?:#\[path\s*=\s*"([^"]+_test_data\.rs)"\]\s*)?mod\s+([a-zA-Z_]\w*_test_data)\s*;/g,
    ),
  ]
  const testConfigurations = source.match(/#\[cfg\((?:test|all\(\s*test\b[^\]]*)\)\]/g) ?? []
  if (wiring.length !== 1 || testConfigurations.length !== wiring.length + testDataWiring.length) {
    return false
  }

  const ownerName = basename(target.file.path)
  const expectedSidecarName = ownerName.replace(/\.rs$/, '_test.rs')
  const declaredSidecarName = wiring[0]?.[1]
  if (declaredSidecarName !== expectedSidecarName) {
    return false
  }

  try {
    await readFile(join(dirname(target.file.path), expectedSidecarName), 'utf8')
  }
  catch {
    context.report({
      evidence: {
        confidence: 'high',
        suggestion: `Create ${expectedSidecarName} beside ${ownerName}, or remove stale test wiring from the owner.`,
      },
      filePath: target.file.path,
      loc: { start: { column: 0, line: source.slice(0, wiring[0]?.index).split('\n').length } },
      message: `${ownerName} registers missing sidecar ${expectedSidecarName}`,
    })
  }

  for (const support of testDataWiring) {
    const supportName = support[1] ?? `${support[2]}.rs`
    try {
      await readFile(join(dirname(target.file.path), supportName), 'utf8')
    }
    catch {
      context.report({
        evidence: {
          confidence: 'high',
          suggestion: `Create private ${supportName} beside ${ownerName}, or remove its stale cfg(test) module declaration.`,
        },
        filePath: target.file.path,
        loc: { start: { column: 0, line: source.slice(0, support.index).split('\n').length } },
        message: `${ownerName} registers missing test-data module ${supportName}`,
      })
    }
  }
  return true
}

async function reviewSideBySideUnitTests(context: RuleContext, target: FileTarget): Promise<void> {
  const source = context.src.getText(await context.src.readFile(target.file))
  if (target.file.path.endsWith('_test.rs')) {
    if (!(await reviewSidecarWiring(context, target))) {
      return
    }
  }
  else {
    if (!source.includes('cfg(test)') && !source.includes('#[test]') && !/\bmod\s+tests\b/.test(source)) {
      return
    }
    if (await reviewCompleteOwnerWiring(context, target, source)) {
      return
    }

    const marker = source.search(/#\[cfg\(test\)\]|#\[(?:tokio::)?test\]|\bmod\s+tests\b/)
    context.report({
      evidence: {
        confidence: 'high',
        suggestion: `Move all test implementation into the matching <stem>_test.rs sidecar and leave only cfg(test), path, and mod tests wiring.`,
      },
      filePath: target.file.path,
      loc: { start: { column: 0, line: source.slice(0, Math.max(marker, 0)).split('\n').length } },
      message: `${basename(target.file.path)} contains test implementation outside its sidecar`,
    })
    return
  }

  const findings = await judgeSource({
    context,
    instructions: sideBySideUnitTestsInstructions,
    operation: 'side-by-side-unit-tests-review',
    prompt: `${sideBySideUnitTestsPrompt}\n\nFile path:\n${target.file.path}`,
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

/**
 * Validates that a sidecar is registered by its same-directory owning module.
 *
 * Triggering workflow:
 *
 * {@link sideBySideUnitTestsRule}
 *   -> {@link reviewSideBySideUnitTests}
 *     -> `_test.rs`
 *       -> {@link reviewSidecarWiring}
 *
 * Upstream:
 * - {@link reviewSideBySideUnitTests}
 *
 * Downstream:
 * - {@link readFile}
 * - {@link RuleContext.report}
 */
async function reviewSidecarWiring(context: RuleContext, target: FileTarget): Promise<boolean> {
  const sidecarName = basename(target.file.path)
  const ownerName = sidecarName.replace(/_test\.rs$/, '.rs')
  const ownerPath = join(dirname(target.file.path), ownerName)
  let ownerSource: string
  try {
    ownerSource = await readFile(ownerPath, 'utf8')
  }
  catch {
    context.report({
      evidence: {
        confidence: 'high',
        suggestion: `Rename this sidecar for an existing production file, or remove the orphaned test target.`,
      },
      filePath: target.file.path,
      loc: { start: { column: 0, line: 1 } },
      message: `${sidecarName} has no same-directory owner ${ownerName}`,
    })
    return false
  }

  const escapedSidecarName = sidecarName.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  const hasExactWiring = new RegExp(
    `#\\[cfg\\((?:test|all\\(\\s*test\\b[^\\]]*)\\)\\]\\s*#\\[path\\s*=\\s*"${escapedSidecarName}"\\]\\s*mod\\s+tests\\s*;`,
  ).test(ownerSource)
  if (!hasExactWiring) {
    context.report({
      evidence: {
        confidence: 'high',
        suggestion: `Register this child from ${ownerName} with #[cfg(test)], #[path = "${sidecarName}"], and mod tests;.`,
      },
      filePath: target.file.path,
      loc: { start: { column: 0, line: 1 } },
      message: `${sidecarName} is not explicitly registered by ${ownerName}`,
    })
    return false
  }
  return true
}
