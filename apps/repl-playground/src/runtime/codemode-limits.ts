/** Execution limits shared by the MCP tool, VM, and agent help. */
export const codeModeLimits = {
  defaultTimeoutMs: 30000,
  maxImageDimension: 1024,
  maxStoreCharacters: 64000,
  maxTextCharacters: 16000,
  maxTimeoutMs: 120000,
  memoryMiB: 64,
} as const
