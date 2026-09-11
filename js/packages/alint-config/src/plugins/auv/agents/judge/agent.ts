import type { ResolvedModel, RuleContext } from '@alint-js/plugin'

export interface JudgeFinding {
  confidence: 'high' | 'low' | 'medium'
  line: number
  message: string
  suggestion: string
}

interface JudgeSourceOptions {
  context: RuleContext
  instructions: string
  operation: string
  prompt: string
  source: string
}

interface ToolResult {
  findings?: unknown
}

const toolName = 'report_findings'
// NOTICE: Some OpenAI-compatible providers occasionally return truncated tool arguments.
// Two repair turns bound duplicate model spend while giving the model enough context
// to correct its own call.
const maxResponseAttempts = 3

export async function judgeSource(options: JudgeSourceOptions): Promise<JudgeFinding[]> {
  const model = await options.context.model()
  const toolResult = await requestToolResult(model, {
    instructions: `${options.instructions}\n\nCall ${toolName} exactly once. Use an empty findings array when there is no qualifying issue.`,
    onUsage: usage => recordUsage(options.context, model, usage),
    prompt: [
      `Review operation: ${options.operation}`,
      options.prompt,
      formatOutputLanguageInstruction(options.context.outputLanguage),
      'Code with line numbers:',
      formatSourceWithLineNumbers(options.source),
    ]
      .filter(Boolean)
      .join('\n\n'),
    signal: options.context.signal,
  })

  return parseFindings(toolResult)
}

function appendToolRepairMessages(messages: unknown[], body: unknown): void {
  const choice = asRecord(asArray(asRecord(body)?.choices)?.[0])
  const message = asRecord(choice?.message)
  const toolCall = asRecord(asArray(message?.tool_calls)?.[0])
  const toolCallId = toolCall?.id

  if (message) {
    messages.push(message)
  }
  if (typeof toolCallId === 'string') {
    messages.push({
      content: `The ${toolName} call arguments were not valid JSON matching the required schema.`,
      role: 'tool',
      tool_call_id: toolCallId,
    })
  }
  messages.push(retryRequest(`Call ${toolName} again exactly once with valid JSON matching the required schema.`))
}

function asArray(value: unknown): undefined | unknown[] {
  return Array.isArray(value) ? value : undefined
}

function asRecord(value: unknown): Record<string, unknown> | undefined {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return undefined
  }

  return value as Record<string, unknown>
}

function chatCompletionsUrl(endpoint: string): string {
  const url = new URL(endpoint)
  const parts = url.pathname.split('/').filter(Boolean)
  url.pathname = `/${[...parts, 'chat', 'completions'].join('/')}`
  return url.toString()
}

function extractToolResult(body: unknown): ToolResult {
  const choice = asRecord(asArray(asRecord(body)?.choices)?.[0])
  const message = asRecord(choice?.message)
  const toolCall = asRecord(asArray(message?.tool_calls)?.[0])
  const toolFunction = asRecord(toolCall?.function)
  const args = toolFunction?.arguments

  if (typeof args === 'string') {
    return JSON.parse(args) as ToolResult
  }

  if (args && typeof args === 'object') {
    return args as ToolResult
  }

  throw new Error(`alint model response did not include a ${toolName} tool call.`)
}

function formatOutputLanguageInstruction(outputLanguage: string | undefined): string {
  if (!outputLanguage) {
    return ''
  }

  return `Write diagnostics and suggestions in ${outputLanguage}.`
}

function formatSourceWithLineNumbers(source: string): string {
  return source
    .split('\n')
    .map((line, index) => `${index + 1} | ${line}`)
    .join('\n')
}

function isFindingInput(value: unknown): value is JudgeFinding {
  if (!value || typeof value !== 'object') {
    return false
  }

  const finding = value as Partial<JudgeFinding>
  return (
    typeof finding.line === 'number'
    && Number.isInteger(finding.line)
    && finding.line > 0
    && typeof finding.message === 'string'
    && typeof finding.suggestion === 'string'
    && (finding.confidence === 'high' || finding.confidence === 'medium' || finding.confidence === 'low')
  )
}

function numberField(record: Record<string, unknown>, key: string): number | undefined {
  const value = record[key]
  return typeof value === 'number' ? value : undefined
}

function parseFindings(result: ToolResult): JudgeFinding[] {
  const findings = asArray(result.findings)
  if (!findings) {
    return []
  }

  const parsed: JudgeFinding[] = []
  const reportedLines = new Set<number>()
  for (const value of findings) {
    if (!isFindingInput(value) || reportedLines.has(value.line)) {
      continue
    }

    reportedLines.add(value.line)
    parsed.push(value)
  }

  return parsed
}

function recordUsage(context: RuleContext, model: ResolvedModel, usage: unknown): void {
  const usageRecord = asRecord(usage)
  if (!usageRecord) {
    return
  }

  context.metering.recordUsage({
    inputTokens: numberField(usageRecord, 'prompt_tokens') ?? numberField(usageRecord, 'inputTokens'),
    modelId: model.id,
    outputTokens: numberField(usageRecord, 'completion_tokens') ?? numberField(usageRecord, 'outputTokens'),
    providerId: model.provider.id,
    totalTokens: numberField(usageRecord, 'total_tokens') ?? numberField(usageRecord, 'totalTokens'),
  })
}

function reportFindingsParameters(): Record<string, unknown> {
  return {
    additionalProperties: false,
    properties: {
      findings: {
        description: 'All warning-level findings. Use an empty array when there is no qualifying issue.',
        items: {
          additionalProperties: false,
          properties: {
            confidence: {
              description: 'Confidence in this finding.',
              enum: ['high', 'medium', 'low'],
              type: 'string',
            },
            line: {
              description: 'Declaration line of the symbol being reported.',
              minimum: 1,
              type: 'number',
            },
            message: {
              description: [
                'Self-contained terminal diagnostic: name the symbol, cite the concrete code shape,',
                'explain why it violates this rule, and state the remediation direction.',
                'The default formatter prints only this field, so never return only a symbol name or category label.',
              ].join(' '),
              type: 'string',
            },
            suggestion: {
              description: 'One concrete remediation direction, under 35 words. This may restate the remediation included in message.',
              type: 'string',
            },
          },
          required: ['line', 'message', 'suggestion', 'confidence'],
          type: 'object',
        },
        type: 'array',
      },
    },
    required: ['findings'],
    type: 'object',
  }
}

async function requestToolResult(
  model: ResolvedModel,
  input: { instructions: string, onUsage: (usage: unknown) => void, prompt: string, signal?: AbortSignal },
): Promise<ToolResult> {
  const messages: unknown[] = [
    { content: input.instructions, role: 'system' },
    { content: input.prompt, role: 'user' },
  ]
  for (let attempt = 1; attempt <= maxResponseAttempts; attempt += 1) {
    const response = await fetch(chatCompletionsUrl(model.provider.endpoint), {
      body: JSON.stringify({
        messages,
        model: model.id,
        temperature: 0,
        // NOTICE: Alibaba thinking-mode models reject required/object tool_choice.
        // Auto remains portable while the system instruction still requires exactly
        // one report call. Remove this workaround when those providers accept the
        // standard forced-function request used by OpenAI-compatible endpoints.
        tool_choice: 'auto',
        tools: [
          {
            function: {
              description: 'Report all warning-level findings for the reviewed source file.',
              name: toolName,
              parameters: reportFindingsParameters(),
              strict: true,
            },
            type: 'function',
          },
        ],
      }),
      headers: {
        'content-type': 'application/json',
        ...model.provider.headers,
      },
      method: 'POST',
      signal: input.signal,
    })

    if (!response.ok) {
      throw new Error(`alint model request failed with HTTP ${response.status}`)
    }

    let body: unknown
    try {
      body = await response.json() as unknown
    }
    catch (error) {
      if (attempt === maxResponseAttempts) {
        throw error
      }
      messages.push(retryRequest('The previous response body was not valid JSON.'))
      continue
    }

    input.onUsage(asRecord(body)?.usage)
    try {
      return extractToolResult(body)
    }
    catch (error) {
      if (attempt === maxResponseAttempts) {
        throw error
      }
      appendToolRepairMessages(messages, body)
    }
  }

  throw new Error('alint model response repair attempts were exhausted')
}

function retryRequest(content: string): Record<string, unknown> {
  return { content, role: 'user' }
}
