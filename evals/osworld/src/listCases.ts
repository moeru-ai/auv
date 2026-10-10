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
 * - an operator wants to audit task, image, PVC, revision, and port assignments
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
    basePvc: item.infrastructure.basePvc,
    id: item.id,
    localAuvPort: item.infrastructure.localAuvPort,
    localSetupPort: item.infrastructure.localSetupPort,
    namespace: item.infrastructure.namespace,
    runtimeImage: item.infrastructure.runtimeImage,
    taskId: item.task.id,
    upstreamRevision: item.upstream.revision,
  }))
  process.stdout.write(`${JSON.stringify(cases, null, 2)}\n`)
}

main()
