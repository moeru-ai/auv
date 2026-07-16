import type { ResolvedModel, RuleContext } from "@alint-js/plugin";

export interface JudgeFinding {
  confidence: "high" | "medium" | "low";
  line: number;
  message: string;
  suggestion: string;
}

interface JudgeSourceOptions {
  context: RuleContext;
  instructions: string;
  operation: string;
  prompt: string;
  source: string;
}

interface ToolResult {
  findings?: unknown;
}

const toolName = "report_findings";

export async function judgeSource(options: JudgeSourceOptions): Promise<JudgeFinding[]> {
  const model = await options.context.model();
  const response = await requestToolResult(model, {
    instructions: options.instructions,
    prompt: [
      `Review operation: ${options.operation}`,
      options.prompt,
      formatOutputLanguageInstruction(options.context.outputLanguage),
      "Code with line numbers:",
      formatSourceWithLineNumbers(options.source),
    ]
      .filter(Boolean)
      .join("\n\n"),
  });

  recordUsage(options.context, model, response.usage);

  return parseFindings(response.toolResult);
}

async function requestToolResult(
  model: ResolvedModel,
  input: { instructions: string; prompt: string },
): Promise<{ toolResult: ToolResult; usage: unknown }> {
  const response = await fetch(chatCompletionsUrl(model.provider.endpoint), {
    body: JSON.stringify({
      messages: [
        { content: input.instructions, role: "system" },
        { content: input.prompt, role: "user" },
      ],
      model: model.id,
      temperature: 0,
      tool_choice: {
        function: { name: toolName },
        type: "function",
      },
      tools: [
        {
          function: {
            description: "Report all warning-level findings for the reviewed source file.",
            name: toolName,
            parameters: reportFindingsParameters(),
            strict: true,
          },
          type: "function",
        },
      ],
    }),
    headers: {
      "content-type": "application/json",
      ...model.provider.headers,
    },
    method: "POST",
    signal: undefined,
  });

  if (!response.ok) {
    throw new Error(`alint model request failed with HTTP ${response.status}`);
  }

  const body = await response.json() as unknown;
  return {
    toolResult: extractToolResult(body),
    usage: asRecord(body)?.usage,
  };
}

function reportFindingsParameters(): Record<string, unknown> {
  return {
    additionalProperties: false,
    properties: {
      findings: {
        description: "All warning-level findings. Use an empty array when there is no qualifying issue.",
        items: {
          additionalProperties: false,
          properties: {
            confidence: {
              description: "Confidence in this finding.",
              enum: ["high", "medium", "low"],
              type: "string",
            },
            line: {
              description: "Declaration line of the symbol being reported.",
              minimum: 1,
              type: "number",
            },
            message: {
              description: "Short diagnostic message naming the reported symbol.",
              type: "string",
            },
            suggestion: {
              description: "One concrete remediation direction, under 35 words.",
              type: "string",
            },
          },
          required: ["line", "message", "suggestion", "confidence"],
          type: "object",
        },
        type: "array",
      },
    },
    required: ["findings"],
    type: "object",
  };
}

function chatCompletionsUrl(endpoint: string): string {
  const url = new URL(endpoint);
  const parts = url.pathname.split("/").filter(Boolean);
  url.pathname = `/${[...parts, "chat", "completions"].join("/")}`;
  return url.toString();
}

function extractToolResult(body: unknown): ToolResult {
  const choice = asRecord(asArray(asRecord(body)?.choices)?.[0]);
  const message = asRecord(choice?.message);
  const toolCall = asRecord(asArray(message?.tool_calls)?.[0]);
  const toolFunction = asRecord(toolCall?.function);
  const args = toolFunction?.arguments;

  if (typeof args === "string") {
    return JSON.parse(args) as ToolResult;
  }

  if (args && typeof args === "object") {
    return args as ToolResult;
  }

  throw new Error(`alint model response did not include a ${toolName} tool call.`);
}

function parseFindings(result: ToolResult): JudgeFinding[] {
  const findings = asArray(result.findings);
  if (!findings) {
    return [];
  }

  const parsed: JudgeFinding[] = [];
  const reportedLines = new Set<number>();
  for (const value of findings) {
    if (!isFindingInput(value) || reportedLines.has(value.line)) {
      continue;
    }

    reportedLines.add(value.line);
    parsed.push(value);
  }

  return parsed;
}

function isFindingInput(value: unknown): value is JudgeFinding {
  if (!value || typeof value !== "object") {
    return false;
  }

  const finding = value as Partial<JudgeFinding>;
  return (
    typeof finding.line === "number"
    && Number.isInteger(finding.line)
    && finding.line > 0
    && typeof finding.message === "string"
    && typeof finding.suggestion === "string"
    && (finding.confidence === "high" || finding.confidence === "medium" || finding.confidence === "low")
  );
}

function recordUsage(context: RuleContext, model: ResolvedModel, usage: unknown): void {
  const usageRecord = asRecord(usage);
  if (!usageRecord) {
    return;
  }

  context.metering.recordUsage({
    inputTokens: numberField(usageRecord, "prompt_tokens") ?? numberField(usageRecord, "inputTokens"),
    modelId: model.id,
    outputTokens: numberField(usageRecord, "completion_tokens") ?? numberField(usageRecord, "outputTokens"),
    providerId: model.provider.id,
    totalTokens: numberField(usageRecord, "total_tokens") ?? numberField(usageRecord, "totalTokens"),
  });
}

function numberField(record: Record<string, unknown>, key: string): number | undefined {
  const value = record[key];
  return typeof value === "number" ? value : undefined;
}

function asRecord(value: unknown): Record<string, unknown> | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return undefined;
  }

  return value as Record<string, unknown>;
}

function asArray(value: unknown): unknown[] | undefined {
  return Array.isArray(value) ? value : undefined;
}

function formatOutputLanguageInstruction(outputLanguage: string | undefined): string {
  if (!outputLanguage) {
    return "";
  }

  return `Write diagnostics and suggestions in ${outputLanguage}.`;
}

function formatSourceWithLineNumbers(source: string): string {
  return source
    .split("\n")
    .map((line, index) => `${index + 1} | ${line}`)
    .join("\n");
}
