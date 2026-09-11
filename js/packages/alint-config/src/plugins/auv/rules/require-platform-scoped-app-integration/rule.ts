import type { AgentTool } from '@alint-js/core/agent'
import type { RuleContext } from '@alint-js/plugin'

import { isAbsolute, relative, resolve, sep } from 'node:path'

import { defineTool, requireAgent } from '@alint-js/core/agent'
import { defineRule } from '@alint-js/plugin'
import { createTools, DEFAULT_IGNORE_PATTERNS } from '@alint-js/tools-fs'

import { platformScopedAppIntegrationPrompt } from './prompt'

const categories = ['automation-placement', 'platform-file', 'scattered-wrapper', 'view-parser-platform'] as const
interface Finding {
  category: FindingCategory
  confidence: 'high' | 'low' | 'medium'
  filePath: string
  line: number
  message: string
  suggestion: string
}

type FindingCategory = (typeof categories)[number]

export const platformScopedAppIntegrationRule = defineRule({
  create: ctx => ({
    async onTargetDirectory(target) {
      const submission = createSubmissionTool(target.path)
      const model = await ctx.model()
      const result = await requireAgent(ctx)({
        instructions: [
          'Inspect the requested Rust crate as a directory-level architecture review.',
          'Use the filesystem tools to inspect the manifest and relevant production source before deciding.',
          'Finish by calling submit_platform_review exactly once, with an empty findings array when the crate complies or is not an app integration.',
        ].join('\n'),
        model,
        prompt: platformScopedAppIntegrationPrompt,
        tools: [
          ...createTools(target.path, { ignore: [...DEFAULT_IGNORE_PATTERNS, '**/target/**'] }),
          submission.tool,
        ],
      })

      recordUsage(ctx, target.path, model, result.usage)

      const findings = submission.getFindings()
      if (!findings) {
        throw new Error('Platform-scoped app integration review ended without a submission.')
      }

      await reportFindings(ctx, target.path, findings)
    },
  }),
})

function createSubmissionTool(cratePath: string): { getFindings: () => Finding[] | undefined, tool: AgentTool } {
  let submitted: Finding[] | undefined
  return {
    getFindings: () => submitted,
    tool: defineTool({
      description: 'Submit all platform-boundary findings, or an empty array when the reviewed crate complies.',
      execute(input) {
        if (submitted) {
          return 'review rejected: findings were already submitted'
        }
        const parsed = parseSubmission(cratePath, input)
        if (typeof parsed === 'string') {
          return `review rejected: ${parsed}`
        }
        submitted = parsed
        return 'review submitted'
      },
      name: 'submit_platform_review',
      parameters: submissionParameters(),
    }),
  }
}

function parseFinding(cratePath: string, value: unknown): Finding | string {
  if (!value || typeof value !== 'object') {
    return 'each finding must be an object'
  }
  const finding = value as Partial<Finding>
  if (!categories.includes(finding.category as FindingCategory)) {
    return 'finding category is invalid'
  }
  if (finding.confidence !== 'high' && finding.confidence !== 'medium' && finding.confidence !== 'low') {
    return 'finding confidence is invalid'
  }
  if (typeof finding.filePath !== 'string' || !finding.filePath || isAbsolute(finding.filePath)) {
    return 'finding filePath must be relative to the reviewed crate'
  }
  if (!Number.isInteger(finding.line) || (finding.line ?? 0) < 1) {
    return 'finding line must be a positive integer'
  }
  if (typeof finding.message !== 'string' || !finding.message.trim()) {
    return 'finding message must not be empty'
  }
  if (typeof finding.suggestion !== 'string' || !finding.suggestion.trim()) {
    return 'finding suggestion must not be empty'
  }

  const absolutePath = resolve(cratePath, finding.filePath)
  const relativePath = relative(resolve(cratePath), absolutePath)
  if (relativePath === '..' || relativePath.startsWith(`..${sep}`) || isAbsolute(relativePath)) {
    return 'finding filePath must stay inside the reviewed crate'
  }

  return {
    category: finding.category as FindingCategory,
    confidence: finding.confidence,
    filePath: relativePath,
    line: finding.line!,
    message: finding.message.trim(),
    suggestion: finding.suggestion.trim(),
  }
}

function parseSubmission(cratePath: string, input: unknown): Finding[] | string {
  if (!input || typeof input !== 'object' || !Array.isArray((input as { findings?: unknown }).findings)) {
    return 'findings must be an array'
  }

  const findings: Finding[] = []
  const seen = new Set<string>()
  for (const value of (input as { findings: unknown[] }).findings) {
    const finding = parseFinding(cratePath, value)
    if (typeof finding === 'string') {
      return finding
    }
    const identity = `${finding.category}:${finding.filePath}:${finding.line}`
    if (!seen.has(identity)) {
      seen.add(identity)
      findings.push(finding)
    }
  }
  return findings
}

function recordUsage(
  ctx: RuleContext,
  cratePath: string,
  model: Awaited<ReturnType<RuleContext['model']>>,
  usage: undefined | { inputTokens?: number, outputTokens?: number, totalTokens?: number },
): void {
  if (!usage) {
    return
  }
  ctx.metering.recordUsage({
    filePath: cratePath,
    inputTokens: usage.inputTokens,
    metadata: { operation: 'platform-scoped-app-integration-review' },
    modelId: model.id,
    outputTokens: usage.outputTokens,
    providerId: model.provider.id,
    ruleId: ctx.id,
    totalTokens: usage.totalTokens,
  })
}

async function reportFindings(ctx: RuleContext, cratePath: string, findings: readonly Finding[]): Promise<void> {
  for (const finding of findings) {
    const filePath = resolve(cratePath, finding.filePath)
    let file
    try {
      file = await ctx.src.readFile(filePath)
    }
    catch {
      continue
    }
    if (finding.line > file.lines.length) {
      continue
    }
    ctx.report({
      evidence: {
        category: finding.category,
        confidence: finding.confidence,
        suggestion: finding.suggestion,
      },
      filePath,
      loc: { start: { column: 0, line: finding.line } },
      message: finding.message,
    })
  }
}

function submissionParameters(): Record<string, unknown> {
  return {
    additionalProperties: false,
    properties: {
      findings: {
        items: {
          additionalProperties: false,
          properties: {
            category: { enum: categories, type: 'string' },
            confidence: { enum: ['high', 'medium', 'low'], type: 'string' },
            filePath: { description: 'Path relative to the reviewed crate.', minLength: 1, type: 'string' },
            line: { minimum: 1, type: 'integer' },
            message: { minLength: 1, type: 'string' },
            suggestion: { minLength: 1, type: 'string' },
          },
          required: ['category', 'confidence', 'filePath', 'line', 'message', 'suggestion'],
          type: 'object',
        },
        type: 'array',
      },
    },
    required: ['findings'],
    type: 'object',
  }
}
