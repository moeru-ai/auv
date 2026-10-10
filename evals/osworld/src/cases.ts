import { readdirSync, readFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

export interface AuvAction {
  /** Command arguments passed as an argv array without a shell. */
  args: string[]
  /** Invoke command registered by AUV, such as `input.keys`. */
  command: string
  /** Bounded desktop settle time after delivery and before the next action. */
  settleMs?: number
}

export interface OsworldCase {
  actions: AuvAction[]
  auv: {
    linuxSha256: string
    macosSha256: string
  }
  caseFile: string
  id: string
  infrastructure: {
    basePvc: string
    localAuvPort: number
    localSetupPort: number
    namespace: string
    runtimeImage: string
  }
  task: OsworldTask
  taskFile: string
  upstream: {
    repository: string
    revision: string
  }
  version: number
}

export interface OsworldTask {
  config: Array<{
    parameters: {
      command: string | string[]
      shell?: boolean
    }
    type: string
  }>
  evaluator: {
    expected: {
      rules: {
        expected: string
      }
      type: string
    }
    func: string
    result: {
      command: string | string[]
      shell?: boolean
      type: string
    }
  }
  id: string
  instruction: string
  snapshot: string
}

interface CaseFile extends Omit<OsworldCase, 'caseFile' | 'task' | 'taskFile'> {
  task: string
}

/**
 * Loads the checked-in OSWorld case catalog in stable filename order.
 *
 * Use when:
 * - Vieval needs one input row per independently provisioned episode
 * - operators need to inspect the exact task and infrastructure pins before a run
 *
 * Expects:
 * - case files under `tasks/cases` to use schema version 1
 * - each case to reference a local, checked-in upstream task JSON file
 *
 * Returns:
 * - validated cases with resolved task bytes and absolute provenance paths
 */
export function loadCases(): OsworldCase[] {
  const packageDirectory = resolve(dirname(fileURLToPath(import.meta.url)), '..')
  const caseDirectory = join(packageDirectory, 'tasks', 'cases')

  return readdirSync(caseDirectory)
    .filter(file => file.endsWith('.json'))
    .sort((left, right) => left.localeCompare(right))
    .map((file) => {
      const caseFile = join(caseDirectory, file)
      const parsed = parseCase(JSON.parse(readFileSync(caseFile, 'utf8')), caseFile)
      const taskFile = resolve(dirname(caseFile), parsed.task)
      const task = parseTask(JSON.parse(readFileSync(taskFile, 'utf8')), taskFile)

      return {
        ...parsed,
        caseFile,
        task,
        taskFile,
      }
    })
}

function isAuvAction(value: unknown): value is AuvAction {
  return isRecord(value)
    && typeof value.command === 'string'
    && isStringArray(value.args)
    && (value.settleMs == null || (Number.isInteger(value.settleMs) && Number(value.settleMs) >= 0 && Number(value.settleMs) <= 5_000))
}

function isExecuteSetup(value: unknown): value is OsworldTask['config'][number] {
  if (!isRecord(value) || value.type !== 'execute' || !isRecord(value.parameters)) {
    return false
  }
  return (typeof value.parameters.command === 'string' || isStringArray(value.parameters.command))
    && (value.parameters.shell == null || typeof value.parameters.shell === 'boolean')
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function isStringArray(value: unknown): value is string[] {
  return Array.isArray(value) && value.every(item => typeof item === 'string')
}

function parseCase(value: unknown, file: string): CaseFile {
  if (!isRecord(value) || value.version !== 1 || typeof value.id !== 'string') {
    throw new Error(`Invalid OSWorld case identity in ${file}.`)
  }
  if (typeof value.task !== 'string' || !isRecord(value.infrastructure) || !isRecord(value.auv) || !isRecord(value.upstream)) {
    throw new Error(`Invalid OSWorld case configuration in ${file}.`)
  }
  if (!Array.isArray(value.actions) || !value.actions.every(isAuvAction)) {
    throw new Error(`Invalid AUV action sequence in ${file}.`)
  }

  const infrastructure = value.infrastructure
  if (
    typeof infrastructure.namespace !== 'string'
    || typeof infrastructure.basePvc !== 'string'
    || typeof infrastructure.runtimeImage !== 'string'
    || !Number.isInteger(infrastructure.localSetupPort)
    || !Number.isInteger(infrastructure.localAuvPort)
  ) {
    throw new TypeError(`Invalid OSWorld infrastructure pins in ${file}.`)
  }
  if (typeof value.auv.linuxSha256 !== 'string' || !/^[a-f0-9]{64}$/.test(value.auv.linuxSha256)) {
    throw new Error(`Invalid AUV Linux digest in ${file}.`)
  }
  if (typeof value.auv.macosSha256 !== 'string' || !/^[a-f0-9]{64}$/.test(value.auv.macosSha256)) {
    throw new Error(`Invalid AUV macOS digest in ${file}.`)
  }
  if (typeof value.upstream.repository !== 'string' || typeof value.upstream.revision !== 'string') {
    throw new TypeError(`Invalid OSWorld upstream provenance in ${file}.`)
  }

  return {
    actions: value.actions,
    auv: {
      linuxSha256: value.auv.linuxSha256,
      macosSha256: value.auv.macosSha256,
    },
    id: value.id,
    infrastructure: {
      basePvc: infrastructure.basePvc as string,
      localAuvPort: infrastructure.localAuvPort as number,
      localSetupPort: infrastructure.localSetupPort as number,
      namespace: infrastructure.namespace as string,
      runtimeImage: infrastructure.runtimeImage as string,
    },
    task: value.task,
    upstream: {
      repository: value.upstream.repository,
      revision: value.upstream.revision,
    },
    version: 1,
  }
}

function parseTask(value: unknown, file: string): OsworldTask {
  if (
    !isRecord(value)
    || typeof value.id !== 'string'
    || typeof value.instruction !== 'string'
    || typeof value.snapshot !== 'string'
    || !Array.isArray(value.config)
  ) {
    throw new Error(`Invalid OSWorld task in ${file}.`)
  }
  if (!isRecord(value.evaluator) || value.evaluator.func !== 'exact_match') {
    throw new Error(`Unsupported OSWorld evaluator in ${file}; this adapter currently accepts exact_match only.`)
  }
  const result = value.evaluator.result
  const expected = value.evaluator.expected
  if (
    !isRecord(result)
    || result.type !== 'vm_command_line'
    || (typeof result.command !== 'string' && !isStringArray(result.command))
    || (result.shell != null && typeof result.shell !== 'boolean')
    || !isRecord(expected)
    || expected.type !== 'rule'
    || !isRecord(expected.rules)
    || typeof expected.rules.expected !== 'string'
  ) {
    throw new Error(`Unsupported OSWorld exact_match shape in ${file}.`)
  }
  if (!value.config.every(isExecuteSetup)) {
    throw new Error(`Unsupported OSWorld setup in ${file}; this adapter currently accepts execute steps only.`)
  }

  return {
    config: value.config,
    evaluator: {
      expected: {
        rules: { expected: expected.rules.expected },
        type: 'rule',
      },
      func: 'exact_match',
      result: {
        command: result.command,
        ...(typeof result.shell === 'boolean' ? { shell: result.shell } : {}),
        type: 'vm_command_line',
      },
    },
    id: value.id,
    instruction: value.instruction,
    snapshot: value.snapshot,
  }
}
