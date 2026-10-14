import type { ModelApi, ModelEndpoint, Prompt } from './agent'

import process from 'node:process'

import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { join, resolve } from 'node:path'

import { errorMessageFrom } from '@moeru/std'
import { x } from 'tinyexec'

import { complete, modelApiFrom, requestFor } from './agent'
import { packageDirectory } from './cases'

interface Check {
  detail: string
  name: string
  ok: boolean
  /** Command an operator can paste to reproduce the check by hand. */
  reproduce?: string
}

// 1x1 transparent PNG; enough to prove the endpoint accepts image input.
const pixel = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII='

/**
 * Checks every prerequisite of `pnpm eval:run` without creating a Pod.
 *
 * Call stack:
 *
 * `pnpm doctor`
 *   -> environment and binary digests
 *   -> `kubectl auth can-i` / node label / base PVC (tinyexec)
 *   -> `pixi run -e osworld python -c "import desktop_env"` (tinyexec)
 *   -> `GET /models`, text and image replies through {@link complete}
 *      (`POST /responses`, or `/chat/completions` with `OSWORLD_OPENAI_API=chat`)
 *   -> `results/doctor.json`
 *
 * Returns:
 * - exit code 1 when any check fails; each failure names a reproduce command
 */
async function main(): Promise<void> {
  const env = process.env
  const checks: Check[] = []
  const add = (check: Check) => {
    checks.push(check)
    process.stdout.write(`${check.ok ? 'PASS' : 'FAIL'}  ${check.name}: ${check.detail}\n${!check.ok && check.reproduce ? `      reproduce: ${check.reproduce}\n` : ''}`)
  }

  for (const name of ['OSWORLD_AUV_LINUX_BINARY', 'OSWORLD_AUV_MACOS_BINARY', 'OSWORLD_KUBECONFIG']) {
    const file = env[name]
    const ok = file != null && existsSync(file)
    add({
      detail: ok ? `${file} sha256=${createHash('sha256').update(readFileSync(file)).digest('hex')}` : `missing (${file ?? 'unset'})`,
      name,
      ok,
    })
  }

  if (env.OSWORLD_AUV_MACOS_BINARY != null && existsSync(env.OSWORLD_AUV_MACOS_BINARY)) {
    const version = await x(env.OSWORLD_AUV_MACOS_BINARY, ['--version'], { throwOnError: false })
    add({ detail: (version.stdout || version.stderr).trim(), name: 'host auv runs', ok: version.exitCode === 0, reproduce: `${env.OSWORLD_AUV_MACOS_BINARY} --version` })
  }

  const namespace = env.OSWORLD_NAMESPACE ?? 'benchmark-osworld-auv'
  const kubectl = ['--kubeconfig', env.OSWORLD_KUBECONFIG ?? '', '--namespace', namespace]
  for (const [name, args, expect] of [
    ['kubectl can create pods', ['auth', 'can-i', 'create', 'pods'], /^yes/],
    ['OSWorld-ready KVM node', ['get', 'nodes', '-l', 'auv.moeru.ai/osworld-ready=true,feature.node.kubernetes.io/kvm=true', '-o', 'name'], /node\//],
    ['base image PVC bound', ['get', 'pvc', 'osworld-v1-base', '-o', 'jsonpath={.status.phase}'], /^Bound/],
  ] as const) {
    const result = await x('kubectl', [...kubectl, ...args], { throwOnError: false, timeout: 30_000 })
    const output = (result.stdout || result.stderr).trim()
    add({ detail: output.split('\n').join(', ') || `exit ${String(result.exitCode)}`, name, ok: expect.test(output), reproduce: `kubectl ${[...kubectl, ...args].join(' ')}` })
  }

  const source = resolve(env.OSWORLD_SOURCE ?? join(packageDirectory, '.cache', 'OSWorld'))
  const bridgeArgs = ['run', '--manifest-path', join(packageDirectory, '..', '..', 'pixi.toml'), '--environment', 'osworld', 'python', '-c', 'import desktop_env.evaluators.metrics, desktop_env.controllers.setup; print("ok")']
  const bridge = await x('pixi', bridgeArgs, { nodeOptions: { cwd: existsSync(source) ? source : packageDirectory, env: { ...env, PYTHONPATH: source } }, throwOnError: false, timeout: 600_000 })
  add({
    detail: bridge.exitCode === 0 ? `upstream evaluators import from ${source}` : (bridge.stderr.trim().split('\n').at(-1) ?? ''),
    name: 'OSWorld Python bridge',
    ok: bridge.exitCode === 0,
    reproduce: `cd ${source} && PYTHONPATH=. pixi ${bridgeArgs.map(arg => (arg.includes(' ') ? `'${arg}'` : arg)).join(' ')}  # run scripts/sync-osworld first if the checkout is missing`,
  })

  const baseURL = (env.OSWORLD_OPENAI_BASE_URL ?? 'http://127.0.0.1:8317/v1').replace(/\/$/, '')
  const apiKey = env.OSWORLD_OPENAI_API_KEY || 'no-key'
  const headers = { 'authorization': `Bearer ${apiKey}`, 'content-type': 'application/json' }
  let api: ModelApi | undefined
  try {
    api = modelApiFrom(env.OSWORLD_OPENAI_API)
    add({ detail: api, name: 'model API (OSWORLD_OPENAI_API)', ok: true })
  }
  catch (error) {
    add({ detail: errorMessageFrom(error) ?? String(error), name: 'model API (OSWORLD_OPENAI_API)', ok: false })
  }
  for (const model of (env.OSWORLD_MODEL ?? 'gpt-5.5').split(',').map(item => item.trim())) {
    const listed = await fetch(`${baseURL}/models`, { headers }).then(async response => [response.status, await response.text()] as const).catch((error: unknown) => [0, String(error)] as const)
    add({
      detail: listed[0] === 200 ? (listed[1].includes(`"${model}"`) ? `${model} listed` : `${model} not in ${listed[1].slice(0, 300)}`) : `HTTP ${listed[0]} ${listed[1].slice(0, 300)}`,
      name: `model list (${baseURL})`,
      ok: listed[0] === 200 && listed[1].includes(`"${model}"`),
      reproduce: `curl -s ${baseURL}/models -H 'authorization: Bearer $OSWORLD_OPENAI_API_KEY'`,
    })
    if (api == null) {
      continue
    }
    // Probes go through the agent's own `complete`, so they exercise the same
    // wire API, request shape, and reply parsing that an episode uses.
    const endpoint: ModelEndpoint = { api, apiKey, baseURL, model }
    const probes: Array<[string, Prompt]> = [
      ['text', { items: [{ role: 'user', text: 'Reply with OK.', type: 'message' }] }],
      ['image', { items: [{ image: `data:image/png;base64,${pixel}`, role: 'user', text: 'Reply with OK if you can see an image.', type: 'message' }] }],
    ]
    for (const [kind, prompt] of probes) {
      const request = requestFor(endpoint, prompt)
      const started = Date.now()
      const reply = await complete(endpoint, prompt, AbortSignal.timeout(120_000))
        .then(({ text }) => ({ text }), (error: unknown) => ({ error: errorMessageFrom(error) ?? String(error) }))
      add({
        detail: 'text' in reply ? `${Date.now() - started}ms reply=${JSON.stringify(reply.text)}` : reply.error.slice(0, 300),
        name: `model reply ${kind} (${api}, ${model})`,
        ok: 'text' in reply && reply.text.length > 0,
        reproduce: `curl -s ${request.url} -H 'content-type: application/json' -H 'authorization: Bearer $OSWORLD_OPENAI_API_KEY' -d '${kind === 'text' ? request.body : '<see src/doctor.ts image payload>'}'`,
      })
    }
  }

  const failed = checks.filter(check => !check.ok)
  if (failed.some(check => check.name.startsWith('model'))) {
    // Only printed on failure: where the local cliproxyapi keeps the state
    // that most often explains a rejected request.
    process.stdout.write([
      'cliproxyapi hints:',
      `  config:   ${join(homedir(), '.cli-proxy-api', 'config.yaml')} (port, api-keys; empty api-keys means no client key)`,
      `  accounts: ${join(homedir(), '.cli-proxy-api')}/*.json (expired OAuth -> re-run cliproxyapi --codex-login)`,
      `  logs:     ${join(homedir(), '.cli-proxy-api', 'logs')}`,
      '  service:  brew services restart cliproxyapi; lsof -iTCP:8317 -sTCP:LISTEN',
      '',
    ].join('\n'))
  }
  const output = join(packageDirectory, 'results')
  mkdirSync(output, { recursive: true })
  writeFileSync(join(output, 'doctor.json'), `${JSON.stringify({ checkedAt: new Date().toISOString(), checks }, null, 2)}\n`)
  process.stdout.write(`${checks.length - failed.length}/${checks.length} checks passed -> results/doctor.json\n`)
  process.exitCode = failed.length === 0 ? 0 : 1
}

void main()
