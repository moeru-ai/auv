import type { AgentStep } from './agent'

import process from 'node:process'

import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join, relative } from 'node:path'

import { packageDirectory } from './cases'
import { terminalUseOf } from './terminalUse'

/** One `result.json` written by `runEpisode`, for evaluated and failed episodes alike. */
interface EpisodeRecord {
  agent?: { lastAction?: string, modelRequests?: number, steps: AgentStep[], usage?: Record<string, number> }
  auvLinuxSha256?: string
  domain: string
  error?: string
  evidence: string
  model: string
  repeat: number
  score?: number
  startedAt: string
  status: 'error' | 'evaluated'
  taskId: string
}

/**
 * Aggregates every episode record into `results/summary.json` and `.md`.
 *
 * Call stack:
 *
 * `pnpm results:summarize [episodesDir]`
 *   -> read `.vieval/episodes/<task>/<run>/result.json`
 *   -> group by task and model
 *   -> write `results/summary.{json,md}`
 *
 * Scoring:
 * - `mean` averages the upstream evaluator score over every attempt; errors count as 0
 * - `pass@k` is 1 when any attempt scored 1
 * - "GUI-only" episodes never targeted, opened, or typed into a terminal
 *   (`src/terminalUse.ts`); they are the ones that measure AUV driving the
 *   task's application. It is recomputed from steps, so older records count.
 */
function main(): void {
  const episodes = process.argv[2] ?? join(packageDirectory, '.vieval', 'episodes')
  const records: EpisodeRecord[] = existsSync(episodes)
    ? readdirSync(episodes).flatMap(task => readdirSync(join(episodes, task))
        .map(run => join(episodes, task, run, 'result.json'))
        .filter(existsSync)
        .map(file => ({ ...JSON.parse(readFileSync(file, 'utf8')), evidence: relative(packageDirectory, join(file, '..')) }) as EpisodeRecord)).filter(record => typeof record.model === 'string')
    : []
  records.sort((left, right) => left.startedAt.localeCompare(right.startedAt))

  const groups = new Map<string, EpisodeRecord[]>()
  for (const record of records) {
    const key = `${record.domain}/${record.taskId}\t${record.model}`
    groups.set(key, [...(groups.get(key) ?? []), record])
  }
  const rows = [...groups.entries()].map(([key, attempts]) => {
    const [task, model] = key.split('\t')
    const scores = attempts.map(attempt => attempt.status === 'evaluated' ? Number(attempt.score) : 0)
    return {
      attempts: attempts.length,
      errors: attempts.filter(attempt => attempt.status === 'error').length,
      mean: scores.reduce((sum, score) => sum + score, 0) / scores.length,
      model,
      passAtK: scores.some(score => score >= 1) ? 1 : 0,
      scores,
      task,
    }
  })

  const passed = records.filter(record => record.status === 'evaluated' && Number(record.score) >= 1).length
  const isPass = (record: EpisodeRecord): boolean => record.status === 'evaluated' && Number(record.score) >= 1
  const routeOf = (record: EpisodeRecord): 'gui' | 'script' | 'terminal' => {
    const use = terminalUseOf(record.agent?.steps ?? [])
    return use.scriptSteps > 0 ? 'script' : use.terminalSteps > 0 ? 'terminal' : 'gui'
  }
  const tally = (subset: EpisodeRecord[]) => ({ episodes: subset.length, passed: subset.filter(isPass).length })
  const routes = {
    gui: tally(records.filter(record => routeOf(record) === 'gui')),
    script: tally(records.filter(record => routeOf(record) === 'script')),
    terminal: tally(records.filter(record => routeOf(record) === 'terminal')),
  }
  const domains = [...new Set(records.map(record => record.domain))].sort().map((domain) => {
    const subset = records.filter(record => record.domain === domain)
    return {
      domain,
      gui: tally(subset.filter(record => routeOf(record) === 'gui')),
      overall: tally(subset),
      terminal: tally(subset.filter(record => routeOf(record) !== 'gui')),
    }
  })
  // Token usage per model, summed over every episode that recorded it.
  const usageByModel: Record<string, Record<string, number>> = {}
  for (const record of records) {
    const usage = (usageByModel[record.model] ??= { episodes: 0, requests: 0 })
    if (record.agent?.usage == null) {
      continue
    }
    usage.episodes += 1
    usage.requests += record.agent.modelRequests ?? 0
    for (const [key, value] of Object.entries(record.agent.usage)) {
      usage[key] = (usage[key] ?? 0) + value
    }
  }
  const rate = ({ episodes, passed }: { episodes: number, passed: number }): string => `${passed}/${episodes}${episodes > 0 ? ` (${(100 * passed / episodes).toFixed(1)}%)` : ''}`
  const errors = records.filter(record => record.status === 'error').length
  const binaries = [...new Set(records.map(record => record.auvLinuxSha256).filter(Boolean))]
  const output = join(packageDirectory, 'results')
  mkdirSync(output, { recursive: true })
  writeFileSync(join(output, 'summary.json'), `${JSON.stringify({ domains, generatedAt: new Date().toISOString(), overall: { auvLinuxSha256: binaries, episodes: records.length, errors, passed, routes, usageByModel }, records, rows }, null, 2)}\n`)
  writeFileSync(join(output, 'summary.md'), [
    '# OSWorld x Vieval results',
    '',
    `Generated ${new Date().toISOString()} from \`${relative(packageDirectory, episodes)}\`. Errors count as score 0.`,
    '',
    `**Overall:** ${passed}/${records.length} episodes scored 1 (${records.length > 0 ? (100 * passed / records.length).toFixed(1) : '0.0'}%), ${errors} harness errors. Guest AUV sha256: ${binaries.map(sha => `\`${sha}\``).join(', ') || '-'}.`,
    '',
    `**By route:** GUI-only ${rate(routes.gui)}; terminal without scripts ${rate(routes.terminal)}; pasted or typed scripts ${rate(routes.script)}. GUI-only episodes never targeted, opened, or typed into a terminal; \`os\` tasks often need one.`,
    '',
    ...Object.entries(usageByModel).filter(([, usage]) => usage.episodes > 0).map(([model, usage]) =>
      `**Usage ${model}:** ${usage.requests} requests over ${usage.episodes} episodes with usage; input ${usage.input_tokens ?? usage.prompt_tokens ?? 0} (cached ${usage['input_tokens_details.cached_tokens'] ?? usage['prompt_tokens_details.cached_tokens'] ?? 0}), output ${usage.output_tokens ?? usage.completion_tokens ?? 0} (reasoning ${usage['output_tokens_details.reasoning_tokens'] ?? usage['completion_tokens_details.reasoning_tokens'] ?? 0}) tokens.`),
    '',
    '| Domain | All | GUI-only | Terminal or script |',
    '| --- | ---: | ---: | ---: |',
    ...domains.map(row => `| ${row.domain} | ${rate(row.overall)} | ${rate(row.gui)} | ${rate(row.terminal)} |`),
    '',
    '| Task | Model | Attempts | Errors | Scores | Mean | pass@k |',
    '| --- | --- | ---: | ---: | --- | ---: | ---: |',
    ...rows.map(row => `| ${row.task} | ${row.model} | ${row.attempts} | ${row.errors} | ${row.scores.join(', ')} | ${row.mean.toFixed(2)} | ${row.passAtK} |`),
    '',
    '## Episodes',
    '',
    '| Started | Task | Model | Repeat | Status | Score | Steps | Route | Last action | Evidence / error |',
    '| --- | --- | --- | ---: | --- | ---: | ---: | --- | --- | --- |',
    ...records.map(record => `| ${record.startedAt} | ${record.taskId.slice(0, 8)} | ${record.model} | ${record.repeat} | ${record.status} | ${record.score ?? '-'} | ${record.agent?.steps.length ?? '-'} | ${routeOf(record)} | ${record.agent?.lastAction ?? '-'} | ${record.error?.replaceAll('|', '\\|').replaceAll('\n', ' ').slice(0, 160) ?? record.evidence} |`),
    '',
  ].join('\n'))
  process.stdout.write(`${rows.length} task/model rows from ${records.length} episodes -> ${relative(process.cwd(), output)}/summary.{json,md}\n`)
}

main()
