import process from 'node:process'

import { readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'

/** One Vieval case: an upstream task plus the domain that groups it. */
export interface OsworldCase {
  domain: string
  /** Task JSON path relative to the package, for reports and audits. */
  file: string
  task: OsworldTask
}

/** Upstream OSWorld task JSON, kept opaque except for the fields AUV reads. */
export interface OsworldTask extends Record<string, unknown> {
  evaluator: { func: string | string[] }
  id: string
  instruction: string
}

export const packageDirectory = resolve(import.meta.dirname, '..')

/**
 * Loads the pinned OSWorld task set synced by `scripts/sync-osworld`.
 *
 * Use when:
 * - Vieval registers one case per task id, prefixed by its domain
 * - an operator lists the denominator before a run
 *
 * Expects:
 * - `tasks/<set>.json` to map each domain to task ids, as upstream
 *   `evaluation_examples/test_*.json` does
 * - optional filters from the environment:
 *   - `OSWORLD_TEST_SET` (default `test_nogdrive`; `test_all` includes Google
 *     Drive tasks that need operator credentials)
 *   - `OSWORLD_DOMAINS` and `OSWORLD_TASK_IDS`, comma-separated
 *
 * Returns:
 * - cases in upstream index order
 */
export function loadCases(environment: NodeJS.ProcessEnv = process.env): OsworldCase[] {
  const set = environment.OSWORLD_TEST_SET ?? 'test_nogdrive'
  const index = readJson(join(packageDirectory, 'tasks', `${set}.json`)) as Record<string, string[]>
  const domains = splitList(environment.OSWORLD_DOMAINS)
  const ids = splitList(environment.OSWORLD_TASK_IDS)

  return Object.entries(index)
    .filter(([domain]) => domains == null || domains.includes(domain))
    .flatMap(([domain, taskIds]) => taskIds
      .filter(id => ids == null || ids.includes(id))
      .map((id) => {
        const file = join('tasks', domain, `${id}.json`)
        const task = readJson(join(packageDirectory, file)) as OsworldTask
        if (task.id !== id || typeof task.instruction !== 'string' || task.evaluator == null) {
          throw new Error(`Invalid OSWorld task ${file}.`)
        }
        return { domain, file, task }
      }))
}

function readJson(file: string): unknown {
  return JSON.parse(readFileSync(file, 'utf8'))
}

function splitList(value: string | undefined): string[] | undefined {
  const items = value?.split(',').map(item => item.trim()).filter(Boolean)
  return items == null || items.length === 0 ? undefined : items
}
