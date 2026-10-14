import type { AgentResult, AgentTelemetry, ModelEndpoint } from './agent'
import type { Harness } from './agents'
import type { OsworldCase } from './cases'

import process from 'node:process'

import { createHash, randomBytes } from 'node:crypto'
import { mkdir, readFile, unlink, writeFile } from 'node:fs/promises'
import { createServer } from 'node:net'
import { join, resolve } from 'node:path'

import { errorCauseFrom, errorMessageFrom, errorStackFrom, sleepWithAbort } from '@moeru/std'

import { AgentRunError } from './agent'
import { runAgent } from './agents'
import { startAuvTools } from './auvTools'
import { packageDirectory } from './cases'
import { observeGuestCommand, recordKubernetesResource, runLoggedCommand, startLoggedCommand } from './executionLog'
import { terminalUseOf } from './terminalUse'

/** Evidence returned from one fresh OSWorld VM episode. */
export interface EpisodeResult {
  agent: AgentResult
  /** SHA-256 of the guest AUV binary, for report provenance. */
  auvLinuxSha256: string
  /** Local directory holding `result.json`, AUV runs, and bridge logs. */
  evidenceDirectory: string
  /** Task-owned Kubernetes Pod name. */
  podName: string
  /** Upstream OSWorld evaluator score in `0..1`. */
  score: number
}

interface ExecuteResponse {
  error: string
  output: string
  returncode: number
  status: string
}

/** Guest ports reached through one `kubectl port-forward`. */
interface Ports {
  auv: number
  chromium: number
  server: number
  vlc: number
}

/**
 * Cluster pins shared by every case. The base PVC holds the read-only
 * OSWorld `System.qcow2`; see VIEVAL.md for its one-time import.
 */
const cluster = {
  basePvc: 'osworld-v1-base',
  namespace: process.env.OSWORLD_NAMESPACE ?? 'benchmark-osworld-auv',
  runtimeImage: 'happysixd/osworld-docker@sha256:0e6497a9295647cf05bf2b2af522fdd79bdeba2737595259cab310a3bcf6baa9',
}

/** Guest port for the AUV daemon; 8080 belongs to OSWorld's VLC getter. */
const guestAuvPort = 9847

// NOTICE: Upstream OSWorld waits 20 s between the last action and
// `env.evaluate()` (`lib_run_single.py`: "Wait for the environment to
// settle"). Evaluating at once read Chrome's Cookies database before Chrome
// committed a deletion (chrome/7b6c7e24 scored 0 with "No sites found" on
// screen). OSWORLD_EVALUATION_SETTLE_SECONDS overrides it.
const evaluationSettleMs = Number(process.env.OSWORLD_EVALUATION_SETTLE_SECONDS ?? 20) * 1000

/** A host file uploaded to one guest path and verified there by SHA-256. */
interface GuestFile {
  bytes: Uint8Array
  guestPath: string
  sha256: string
}

/**
 * Joins an error's message with its `cause` chain.
 *
 * NOTICE: Node's `fetch` reports every transport failure as `fetch failed`
 * and keeps the reason (`ECONNRESET`, `UND_ERR_SOCKET`, ...) in `cause`.
 * Vieval prints nothing until the run ends, so `result.json` is the only
 * place where the reason survives.
 */
export function describeError(error: unknown): string {
  const messages: string[] = []
  for (let current = error; current != null && messages.length < 5; current = errorCauseFrom(current)) {
    messages.push(errorMessageFrom(current) ?? String(current))
  }
  return messages.join(': ') || 'unknown error'
}

/**
 * Runs one OSWorld task in a fresh task-owned Kubernetes Pod.
 *
 * Call stack:
 *
 * Vieval `caseOf`
 *   -> {@link runEpisode}
 *     -> `kubectl create` / `kubectl port-forward`
 *     -> guest AUV daemon + paired host AUV client
 *     -> `bridge/osworld_bridge.py setup` (upstream SetupController)
 *     -> {@link runAgent} (model chooses typed AUV actions)
 *     -> `bridge/osworld_bridge.py evaluate` (upstream getters and metrics)
 *     -> UID-scoped Pod deletion
 *
 * Expects:
 * - `OSWORLD_KUBECONFIG`, `OSWORLD_AUV_LINUX_BINARY`, `OSWORLD_AUV_MACOS_BINARY`
 * - `pixi install -e osworld` and `scripts/sync-osworld` to have run
 *
 * Returns:
 * - the upstream score plus AUV Run and agent evidence
 */
export async function runEpisode(
  item: OsworldCase,
  endpoint: ModelEndpoint,
  { harness, repeat, signal, telemetry }: { harness: Harness, repeat: number, signal: AbortSignal, telemetry?: AgentTelemetry },
): Promise<EpisodeResult> {
  const kubeconfig = requiredEnvironment('OSWORLD_KUBECONFIG')
  const auvBinary = resolve(requiredEnvironment('OSWORLD_AUV_LINUX_BINARY'))
  const macosAuvBinary = resolve(requiredEnvironment('OSWORLD_AUV_MACOS_BINARY'))
  const binary = await guestFile(auvBinary, '/home/user/auv')
  // `scripts/build-auv-linux` writes the OCR runtime next to the binary.
  const runtime = await guestFile(`${auvBinary}.runtime.tar.gz`, '/home/user/auv-runtime.tar.gz')
  const auvLinuxSha256 = binary.sha256

  const caseId = item.task.id
  const podName = `osworld-${caseId.slice(0, 8)}-${randomBytes(3).toString('hex')}`
  const startedAt = new Date()
  const evidenceDirectory = join(packageDirectory, '.vieval', 'episodes', caseId, `${startedAt.toISOString().replaceAll(':', '-')}-${endpoint.model}-${harness}-r${repeat}`)
  await mkdir(evidenceDirectory, { recursive: true })
  // `scripts/summarize.ts` aggregates these records, including errors that
  // never reached the evaluator, so the denominator stays honest.
  const maxSteps = Number(process.env.OSWORLD_MAX_STEPS ?? 15)
  const record = { api: endpoint.api, auvRuntimeSha256: runtime.sha256, domain: item.domain, harness, maxSteps, model: endpoint.model, podName, repeat, startedAt: startedAt.toISOString(), taskId: caseId }

  const kubectl = ['--kubeconfig', kubeconfig, '--namespace', cluster.namespace]
  const log = { caseId, podName }
  let podCreated = false
  let portForward: ReturnType<typeof startLoggedCommand> | undefined
  // Kept outside `try` so an evaluator failure still records what the agent did.
  let agent: AgentResult | undefined
  let auvTools: Awaited<ReturnType<typeof startAuvTools>> | undefined
  const profilesFile = join(evidenceDirectory, 'device-profiles.json')

  try {
    const manifest = podManifest(podName, caseId)
    await checked('kubectl', [...kubectl, 'create', '--filename', '-'], { signal, stdin: `${JSON.stringify(manifest)}\n`, timeout: 60_000 }, log)
    podCreated = true
    recordKubernetesResource({ action: 'create', apiVersion: 'v1', caseId, kind: 'Pod', manifest, name: podName, namespace: cluster.namespace, outcome: 'succeeded' })
    await checked('kubectl', [...kubectl, 'wait', '--for=jsonpath={.status.phase}=Running', '--timeout=8m', `pod/${podName}`], { signal, timeout: 500_000 }, log)

    const ports: Ports = { auv: await freePort(), chromium: await freePort(), server: await freePort(), vlc: await freePort() }
    const setupEndpoint = `http://127.0.0.1:${ports.server}`
    portForward = await connectSetup(kubectl, podName, ports, setupEndpoint, { caseId, evidenceDirectory }, signal)

    await installAuv(setupEndpoint, binary, runtime, log, signal)
    const deviceId = await pairAuv(setupEndpoint, macosAuvBinary, ports.auv, profilesFile, log, signal)
    auvTools = await startAuvTools({
      context: log,
      deviceId,
      env: { ...process.env, AUV_CONFIG_PROFILES_FILE: profilesFile },
      executable: macosAuvBinary,
      stderrFile: join(evidenceDirectory, 'auv-mcp.stderr.log'),
      storeRoot: join(evidenceDirectory, 'auv-runs'),
    }, signal)

    const setup = await bridge('setup', item, ports, evidenceDirectory, undefined, log, signal)
    if (setup.ok !== true) {
      throw new Error(`OSWorld setup failed for ${caseId}.`)
    }
    agent = await runAgent(harness, item.task.instruction, endpoint, auvTools, {
      maxSteps,
      signal,
      telemetry,
    })
    // Let applications persist state before the evaluator reads it; see `evaluationSettleMs`.
    await sleepWithAbort(evaluationSettleMs, signal)
    const evaluation = await bridge('evaluate', item, ports, evidenceDirectory, agent.lastAction, log, signal)
    const score = Number(evaluation.score)

    await writeResult(evidenceDirectory, { ...record, agent, auvLinuxSha256, score, status: 'evaluated', terminalUse: terminalUseOf(agent.steps) })
    return { agent, auvLinuxSha256, evidenceDirectory, podName, score }
  }
  catch (error) {
    agent ??= error instanceof AgentRunError ? error.partial : undefined
    await writeResult(evidenceDirectory, { ...record, agent, error: describeError(error), errorStack: errorStackFrom(error), status: 'error', terminalUse: agent == null ? undefined : terminalUseOf(agent.steps) })
    throw error
  }
  finally {
    auvTools?.close()
    portForward?.process.kill('SIGTERM')
    await unlink(profilesFile).catch(() => undefined)
    await unlink(`${profilesFile}.lock`).catch(() => undefined)
    if (podCreated && process.env.OSWORLD_KEEP_POD !== 'true') {
      const deleted = await runLoggedCommand('kubectl', [...kubectl, 'delete', `pod/${podName}`, '--ignore-not-found=true', '--wait=true', '--timeout=2m'], { timeout: 130_000 }, { ...log, runtime: 'host', tool: 'kubectl' })
      recordKubernetesResource({ action: 'delete', apiVersion: 'v1', caseId, exitCode: deleted.exitCode, kind: 'Pod', name: podName, namespace: cluster.namespace, outcome: deleted.exitCode === 0 ? 'succeeded' : 'failed' })
    }
  }
}

function assertExecuteSucceeded(result: ExecuteResponse, action: string): void {
  if (result.status !== 'success' || result.returncode !== 0) {
    throw new Error(`Failed to ${action}: ${result.error || result.output}`)
  }
}

/**
 * Runs one upstream OSWorld setup or evaluation through the Python bridge.
 *
 * The bridge runs with the pinned checkout as its working directory because
 * upstream getters resolve some settings files relative to the repository.
 */
async function bridge(
  command: 'evaluate' | 'setup',
  item: OsworldCase,
  ports: Ports,
  evidenceDirectory: string,
  lastAction: string | undefined,
  log: { caseId: string, podName: string },
  signal: AbortSignal,
): Promise<Record<string, unknown>> {
  const source = resolve(process.env.OSWORLD_SOURCE ?? join(packageDirectory, '.cache', 'OSWorld'))
  const request = { cacheDir: join(evidenceDirectory, 'osworld-cache'), command, lastAction, ports, task: item.task }
  const result = await runLoggedCommand('pixi', [
    'run',
    '--manifest-path',
    join(packageDirectory, '..', '..', 'pixi.toml'),
    '--environment',
    'osworld',
    'python',
    join(packageDirectory, 'bridge', 'osworld_bridge.py'),
  ], {
    nodeOptions: { cwd: source, env: { ...process.env, OSWORLD_SOURCE: source } },
    signal,
    stdin: JSON.stringify(request),
    timeout: 15 * 60_000,
  }, { ...log, runtime: 'host', tool: 'osworld' })
  // The stdout JSON (`{"ok": …}` / `{"score": …}`) is the evaluator verdict;
  // keep it next to the upstream log so a score can be audited offline.
  await writeFile(join(evidenceDirectory, `bridge-${command}.log`), `${result.stderr}\n--- stdout (exit ${String(result.exitCode)}) ---\n${result.stdout}\n`)
  if (result.exitCode !== 0) {
    throw new Error(`OSWorld bridge ${command} failed (${String(result.exitCode)}); see bridge-${command}.log.`)
  }
  return JSON.parse(result.stdout) as Record<string, unknown>
}

async function checked(
  command: string,
  args: string[],
  options: { signal?: AbortSignal, stdin?: string, timeout: number },
  log: { caseId: string, podName: string },
): Promise<void> {
  const result = await runLoggedCommand(command, args, options, { ...log, runtime: 'host', tool: 'kubectl' })
  if (result.exitCode !== 0) {
    throw new Error(`${command} ${args.join(' ')} failed (${String(result.exitCode)}): ${result.stderr || result.stdout}`)
  }
}

/**
 * Forwards every guest port OSWorld and AUV need, retrying while QEMU boots.
 * kubectl exits when the guest port is not yet listening, so each attempt
 * starts a fresh forward and probes the setup API.
 */
async function connectSetup(
  kubectl: string[],
  podName: string,
  ports: Ports,
  endpoint: string,
  { caseId, evidenceDirectory }: { caseId: string, evidenceDirectory: string },
  signal: AbortSignal,
): Promise<ReturnType<typeof startLoggedCommand>> {
  for (let attempt = 0; attempt < 120; attempt += 1) {
    const forward = startLoggedCommand('kubectl', [
      ...kubectl,
      'port-forward',
      `pod/${podName}`,
      `${ports.server}:5000`,
      `${ports.chromium}:9222`,
      `${ports.vlc}:8080`,
      `${ports.auv}:${guestAuvPort}`,
    ], { signal }, { caseId, podName, runtime: 'host', tool: 'kubectl' })
    let ended = false
    let stderr = ''
    // NOTICE: Both handlers are required. The returned forward is never
    // awaited, so when the case aborts, a rejection here was unhandled and
    // crashed the whole Vieval run (test_nogdrive, 2026-10-11).
    const completion = forward.completion.then((output) => {
      ended = true
      stderr = output.stderr
    }, () => {
      ended = true
    })

    for (let probe = 0; probe < 4; probe += 1) {
      await sleepWithAbort(1_000, signal)
      if (ended) {
        break
      }
      const response = await fetch(`${endpoint}/screenshot`, { signal }).catch(() => undefined)
      if (response?.ok === true) {
        return forward
      }
    }
    forward.process.kill('SIGTERM')
    await completion
    // A Pod that left Running never serves the setup API again; retrying for
    // eight minutes hid why (libreoffice_impress/550ce7e7, 2026-10-12).
    if (stderr.includes('pod is not running')) {
      await savePodDiagnostics(kubectl, podName, evidenceDirectory, caseId)
      throw new Error(`Episode Pod ${podName} stopped running before the setup API answered; see pod-diagnostics.log.`)
    }
  }
  throw new Error('OSWorld setup API did not become ready within eight minutes.')
}

async function execute(endpoint: string, body: { command: string | string[], shell?: boolean }, signal: AbortSignal): Promise<ExecuteResponse> {
  const response = await fetch(`${endpoint}/setup/execute`, {
    body: JSON.stringify(body),
    headers: { 'content-type': 'application/json' },
    method: 'POST',
    signal,
  })
  if (!response.ok) {
    throw new Error(`OSWorld execute failed with HTTP ${response.status}: ${await response.text()}`)
  }
  return await response.json() as ExecuteResponse
}

/** Reserves an unused local TCP port for one forwarded guest port. */
async function freePort(): Promise<number> {
  return await new Promise((resolvePort, reject) => {
    const server = createServer()
    server.once('error', reject)
    server.listen(0, '127.0.0.1', () => {
      const address = server.address()
      server.close(() => resolvePort(typeof address === 'object' && address != null ? address.port : 0))
    })
  })
}

async function guestFile(hostPath: string, guestPath: string): Promise<GuestFile> {
  const bytes = await readFile(hostPath).catch((error: unknown) => {
    throw new Error(`Cannot read ${hostPath}; build it with scripts/build-auv-linux.`, { cause: error })
  })
  return { bytes, guestPath, sha256: createHash('sha256').update(bytes).digest('hex') }
}

/**
 * Uploads AUV and its OCR runtime, then starts the guest daemon.
 *
 * NOTICE: The binary's RUNPATH is `$ORIGIN/auv-runtime/lib`, so every guest
 * `auv` call finds the bundled Tesseract and Leptonica; only the daemon needs
 * `TESSDATA_PREFIX`. Nothing is installed through apt, which raced with
 * PackageKit on fresh VMs (82 of 124 episodes waited on the apt lock).
 */
async function installAuv(
  endpoint: string,
  binary: GuestFile,
  runtime: GuestFile,
  log: { caseId: string, podName: string },
  signal: AbortSignal,
): Promise<void> {
  await uploadGuestFile(endpoint, binary, signal)
  await uploadGuestFile(endpoint, runtime, signal)

  const verified = (file: GuestFile) => `test "$(sha256sum ${file.guestPath} | cut -d' ' -f1)" = "${file.sha256}"`
  const launchCommand = [
    `chmod 0755 ${binary.guestPath}`,
    verified(binary),
    verified(runtime),
    'rm -rf /home/user/auv-runtime && mkdir -p /home/user/auv-runtime',
    `tar -xzf ${runtime.guestPath} -C /home/user/auv-runtime`,
    'rm -f /home/user/auv.sock',
    `nohup env DISPLAY=:0 XDG_SESSION_TYPE=x11 TESSDATA_PREFIX=/home/user/auv-runtime/tessdata ${binary.guestPath} serve --listen unix:///home/user/auv.sock --listen http://0.0.0.0:${guestAuvPort} --pairing-store /home/user/.local/share/auv-osworld/pairings.json --store-root /home/user/.local/share/auv-osworld --no-register </dev/null >/tmp/auv.log 2>&1 &`,
  ].join(' && ')
  const launch = await observeGuestCommand('/bin/sh', ['-c', launchCommand], { ...log, tool: 'auv' }, async () => await execute(endpoint, {
    command: launchCommand,
    shell: true,
  }, signal), result => result.returncode)
  assertExecuteSucceeded(launch, 'launch the guest-local AUV daemon')

  for (let attempt = 0; attempt < 60; attempt += 1) {
    const probe = await execute(endpoint, { command: ['test', '-S', '/home/user/auv.sock'] }, signal)
    if (probe.returncode === 0) {
      return
    }
    await sleepWithAbort(1_000, signal)
  }
  throw new Error('Guest-local AUV daemon did not create its Unix socket within 60 seconds.')
}

/** Pairs the host AUV client with the guest daemon and returns the device id. */
async function pairAuv(
  setupEndpoint: string,
  binary: string,
  localAuvPort: number,
  profilesFile: string,
  log: { caseId: string, podName: string },
  signal: AbortSignal,
): Promise<string> {
  const tokenCommand = ['env', 'AUV_ENDPOINT=unix:///home/user/auv.sock', '/home/user/auv', 'devices', 'pair', 'create-token']
  const tokenResult = await observeGuestCommand(tokenCommand[0], tokenCommand.slice(1), { ...log, tool: 'auv' }, async () => await execute(setupEndpoint, {
    command: tokenCommand,
  }, signal), result => result.returncode)
  assertExecuteSucceeded(tokenResult, 'create the AUV pairing token')
  const token = tokenResult.output.trim()
  if (!/^[a-f0-9]{32}$/.test(token)) {
    throw new Error('Guest AUV returned an invalid pairing token.')
  }

  const connected = await runLoggedCommand(binary, [
    'devices',
    'pair',
    '--endpoint',
    `http://127.0.0.1:${localAuvPort}`,
    'connect',
    '--token-stdin',
    '--label',
    'Vieval OSWorld',
    '--profile',
    'osworld-episode',
    '--json',
  ], {
    nodeOptions: { env: { ...process.env, AUV_CONFIG_PROFILES_FILE: profilesFile } },
    signal,
    stdin: token,
    timeout: 60_000,
  }, { ...log, runtime: 'host', tool: 'auv' })
  if (connected.exitCode !== 0) {
    throw new Error(`AUV pairing failed (${String(connected.exitCode)}): ${connected.stderr || connected.stdout}`)
  }
  const enrollment = JSON.parse(connected.stdout) as { device_id?: unknown }
  if (typeof enrollment.device_id !== 'string') {
    throw new TypeError(`AUV pairing returned an unexpected response: ${connected.stdout}`)
  }
  return enrollment.device_id
}

function podManifest(podName: string, caseId: string): Record<string, unknown> {
  return {
    apiVersion: 'v1',
    kind: 'Pod',
    metadata: {
      labels: {
        'app.kubernetes.io/managed-by': 'vieval',
        'app.kubernetes.io/name': 'osworld-episode',
        'vieval.dev/case': caseId,
      },
      name: podName,
      namespace: cluster.namespace,
    },
    spec: {
      containers: [{
        env: [
          { name: 'DISK_SIZE', value: '32G' },
          { name: 'RAM_SIZE', value: '8G' },
          { name: 'CPU_CORES', value: '4' },
          // NOTICE: `kubectl port-forward` dials localhost in the Pod, which only
          // user-mode `hostfwd` (`USER_PORTS`) serves. qemu-docker defaults to
          // NAT (iptables on eth0) and drops to user mode only when NAT fails,
          // so pin it. Ports: setup API, Chromium CDP, VLC, AUV daemon.
          { name: 'NETWORK', value: 'user' },
          { name: 'USER_PORTS', value: `5000,9222,8080,${guestAuvPort}` },
        ],
        image: cluster.runtimeImage,
        imagePullPolicy: 'IfNotPresent',
        name: 'qemu',
        resources: {
          limits: { cpu: '8', memory: '12Gi' },
          requests: { cpu: '5m', memory: '8Gi' },
        },
        securityContext: { privileged: true },
        volumeMounts: [
          { mountPath: '/System.qcow2', name: 'image', readOnly: true, subPath: 'System.qcow2' },
          { mountPath: '/dev/kvm', name: 'kvm' },
        ],
      }],
      nodeSelector: {
        // NOTICE: KVM availability does not prove that the pinned OSWorld
        // guest reaches its setup API. Operators add this label only after a
        // live episode probe; adding automatic qualification is deferred until
        // the cluster has a reusable node-conformance controller.
        'auv.moeru.ai/osworld-ready': 'true',
        'feature.node.kubernetes.io/kvm': 'true',
        'kubernetes.io/arch': 'amd64',
      },
      restartPolicy: 'Never',
      terminationGracePeriodSeconds: 30,
      volumes: [
        { name: 'image', persistentVolumeClaim: { claimName: cluster.basePvc, readOnly: true } },
        { hostPath: { path: '/dev/kvm', type: 'CharDevice' }, name: 'kvm' },
      ],
    },
  }
}

function requiredEnvironment(name: string): string {
  const value = process.env[name]
  if (value == null || value.length === 0) {
    throw new Error(`${name} must name the local input required for an OSWorld run.`)
  }
  return value
}

/** Saves the Pod status and QEMU log next to the episode before the Pod is deleted. */
async function savePodDiagnostics(kubectl: string[], podName: string, evidenceDirectory: string, caseId: string): Promise<void> {
  const log = { caseId, podName, runtime: 'host', tool: 'kubectl' } as const
  const status = await runLoggedCommand('kubectl', [...kubectl, 'get', `pod/${podName}`, '--output', 'jsonpath={.status}'], { timeout: 30_000 }, log)
  const logs = await runLoggedCommand('kubectl', [...kubectl, 'logs', `pod/${podName}`, '--tail', '300'], { timeout: 30_000 }, log)
  await writeFile(join(evidenceDirectory, 'pod-diagnostics.log'), `--- status ---\n${status.stdout}${status.stderr}\n--- qemu log (tail) ---\n${logs.stdout}${logs.stderr}\n`)
}

/**
 * Writes one file to the guest through the OSWorld setup API.
 *
 * NOTICE: kubectl port-forward can time out opening a stream for a large
 * request while the forward stays up (chrome/a96b564e, 2026-10-11:
 * `write EPIPE`). An upload overwrites one path, so transport failures are
 * retried; HTTP errors are not.
 */
async function uploadGuestFile(endpoint: string, file: GuestFile, signal: AbortSignal): Promise<void> {
  const form = new FormData()
  form.append('file_path', file.guestPath)
  form.append('file_data', new Blob([new Uint8Array(file.bytes).buffer]), file.guestPath.split('/').at(-1))
  let upload: Response | undefined
  for (let attempt = 1; upload == null; attempt += 1) {
    upload = await fetch(`${endpoint}/setup/upload`, { body: form, method: 'POST', signal }).catch(async (error: unknown) => {
      if (attempt >= 3 || signal.aborted) {
        throw error
      }
      await sleepWithAbort(3_000, signal)
      return undefined
    })
  }
  if (!upload.ok) {
    throw new Error(`OSWorld rejected the upload of ${file.guestPath} with HTTP ${upload.status}: ${await upload.text()}`)
  }
}

async function writeResult(evidenceDirectory: string, result: Record<string, unknown>): Promise<void> {
  await writeFile(join(evidenceDirectory, 'result.json'), `${JSON.stringify({ ...result, finishedAt: new Date().toISOString() }, null, 2)}\n`)
}
