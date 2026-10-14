import process from 'node:process'

import { loadCases } from './cases'

/**
 * Prints the case catalog without creating Kubernetes resources.
 *
 * Call stack:
 *
 * `pnpm cases:list`
 *   -> {@link loadCases}
 *   -> `process.stdout.write(...)`
 *
 * Use when:
 * - an operator wants to audit the selected task set and its evaluators
 * - CI needs to prove that every case configuration can be discovered and parsed
 *
 * Expects:
 * - no cluster access or AUV binary
 *
 * Returns:
 * - one JSON array on stdout
 */
function main(): void {
  const cases = loadCases().map(item => ({
    domain: item.domain,
    evaluator: item.task.evaluator.func,
    file: item.file,
    id: item.task.id,
  }))
  process.stdout.write(`${JSON.stringify(cases, null, 2)}\n`)
}

main()
