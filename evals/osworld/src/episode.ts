import type { OsworldCase } from './cases'

import process from 'node:process'

import { createHash } from 'node:crypto'
import { mkdir, readFile, unlink, writeFile } from 'node:fs/promises'
import { basename, join, resolve } from 'node:path'

import { observeGuestCommand, recordKubernetesResource, runLoggedCommand, startLoggedCommand } from './executionLog'
import { installRuntimeLibraries } from './guestSetup'

/** Evidence returned from one fresh OSWorld VM episode. */
export interface EpisodeResult {
  /** Local copy of the final AUV capture. */
  artifactPath: string
  /** SHA-256 declared by AUV and independently checked after download. */
  artifactSha256: string
  /** Raw output from the pinned OSWorld evaluator command. */
  evaluatorOutput: string
  /** Task-owned Kubernetes Pod name. */
  podName: string
  /** AUV Run IDs for the before capture, actions, and final capture. */
  runIds: string[]
  /** Exact-match score in the normalized `0..1` range. */
  score: number
}

interface ExecuteResponse {
  error: string
  output: string
  returncode: number
  status: string
}

interface InvokeResponse {
  artifacts: Array<{
    file_path: string
    sha256: string
  }>
  run_id: string
  status: string
}

interface PairedAuv {
  binary: string
  deviceId: string
  profilesFile: string
  storeRoot: string
}

/**
 * Runs one checked-in OSWorld case in a fresh task-owned Kubernetes Pod.
 *
 * Call stack:
 *
 * Vieval `casesFromInputs`
 *   -> {@link runEpisode}
 *     -> `kubectl create` / direct Pod port-forward
 *     -> OSWorld setup API
 *     -> paired Linux daemon and same-commit macOS AUV typed invoke commands
 *     -> pinned OSWorld evaluator
 *     -> AUV artifact download
 *     -> UID-scoped task-owned Pod deletion
 *
 * Use when:
 * - Vieval should own the benchmark denominator and case scheduling
 * - each case needs a disposable qcow2 overlay over a read-only OSWorld base PVC
 *
 * Expects:
 * - `OSWORLD_KUBECONFIG` and `OSWORLD_AUV_LINUX_BINARY` to name readable files
 * - the case catalog to pin the image digest, base PVC, task, and AUV digest
 * - Node Feature Discovery to label nodes where KVM is enabled
 *
 * Returns:
 * - official evaluator output plus AUV Run and artifact evidence
 */
export async function runEpisode(item: OsworldCase, signal: AbortSignal): Promise<EpisodeResult> {
  const kubeconfig = requiredEnvironment('OSWORLD_KUBECONFIG')
  const auvBinary = resolve(requiredEnvironment('OSWORLD_AUV_LINUX_BINARY'))
  const macosAuvBinary = resolve(requiredEnvironment('OSWORLD_AUV_MACOS_BINARY'))
  const binaryBytes = await readFile(auvBinary)
  const binarySha256 = createHash('sha256').update(binaryBytes).digest('hex')
  if (binarySha256 !== item.auv.linuxSha256) {
    throw new Error(`AUV Linux digest mismatch: expected ${item.auv.linuxSha256}, received ${binarySha256}.`)
  }
  const macosBinarySha256 = createHash('sha256').update(await readFile(macosAuvBinary)).digest('hex')
  if (macosBinarySha256 !== item.auv.macosSha256) {
    throw new Error(`AUV macOS digest mismatch: expected ${item.auv.macosSha256}, received ${macosBinarySha256}.`)
  }

  const podName = `vieval-osworld-${item.task.id.slice(0, 8)}-${item.infrastructure.localSetupPort}`
  const evidenceDirectory = join(
    resolve(import.meta.dirname, '..', '.vieval', 'episodes'),
    item.id,
    new Date().toISOString().replaceAll(':', '-'),
  )
  await mkdir(evidenceDirectory, { recursive: true })

  const kubectl = ['--kubeconfig', kubeconfig, '--namespace', item.infrastructure.namespace]
  let podCreated = false
  let portForward: ReturnType<typeof startLoggedCommand>['process'] | undefined
  let portForwardCompletion: Promise<void> | undefined
  let profilesFile: string | undefined

  try {
    const manifest = podManifest(item, podName)
    await checked('kubectl', [...kubectl, 'create', '--filename', '-'], {
      signal,
      stdin: `${JSON.stringify(manifest)}\n`,
      timeout: 60_000,
    }, { caseId: item.id, podName, tool: 'kubectl' })
    podCreated = true
    recordKubernetesResource({
      action: 'create',
      apiVersion: 'v1',
      caseId: item.id,
      kind: 'Pod',
      manifest,
      name: podName,
      namespace: item.infrastructure.namespace,
      outcome: 'succeeded',
    })

    await checked('kubectl', [
      ...kubectl,
      'wait',
      '--for=jsonpath={.status.phase}=Running',
      '--timeout=8m',
      `pod/${podName}`,
    ], { signal, timeout: 500_000 }, { caseId: item.id, podName, tool: 'kubectl' })

    const setupEndpoint = `http://127.0.0.1:${item.infrastructure.localSetupPort}`
    const activeForward = await connectSetup(
      kubectl,
      podName,
      item.infrastructure.localSetupPort,
      item.infrastructure.localAuvPort,
      setupEndpoint,
      item.id,
      signal,
    )
    portForward = activeForward.process
    portForwardCompletion = activeForward.completion

    await installAuv(setupEndpoint, auvBinary, binaryBytes, item.auv.linuxSha256, item.id, podName, signal)
    await prepareTask(setupEndpoint, item, signal)
    const pairedAuv = await pairAuv(setupEndpoint, macosAuvBinary, item.infrastructure.localAuvPort, evidenceDirectory, item.id, podName, signal)
    profilesFile = pairedAuv.profilesFile

    const runIds: string[] = []
    const beforeCapture = await invokeAuv(pairedAuv, 'display.capture', [], item.id, podName, signal)
    runIds.push(beforeCapture.run_id)

    for (const action of item.actions) {
      const result = await invokeAuv(pairedAuv, action.command, action.args, item.id, podName, signal)
      runIds.push(result.run_id)
      if (action.settleMs != null && action.settleMs > 0) {
        await delay(action.settleMs, signal)
      }
    }

    const finalCapture = await invokeAuv(pairedAuv, 'display.capture', [], item.id, podName, signal)
    runIds.push(finalCapture.run_id)
    const artifact = finalCapture.artifacts[0]
    if (artifact == null) {
      throw new Error(`Final AUV capture ${finalCapture.run_id} did not contain an artifact.`)
    }
    const artifactPath = artifact.file_path
    const artifactBytes = await readFile(artifactPath)
    const downloadedSha256 = createHash('sha256').update(artifactBytes).digest('hex')
    if (downloadedSha256 !== artifact.sha256) {
      throw new Error(`Final AUV artifact digest mismatch: expected ${artifact.sha256}, received ${downloadedSha256}.`)
    }
    const evaluator = item.task.evaluator
    const evaluation = await execute(setupEndpoint, {
      command: evaluator.result.command,
      shell: evaluator.result.shell,
    }, signal)
    const expected = evaluator.expected.rules.expected
    const score = evaluation.returncode === 0 && evaluation.output === expected ? 1 : 0
    await writeFile(join(evidenceDirectory, 'result.json'), `${JSON.stringify({
      artifactPath,
      artifactSha256: downloadedSha256,
      evaluatorOutput: evaluation.output,
      podName,
      runIds,
      score,
      taskId: item.task.id,
    }, null, 2)}\n`)

    portForward.kill('SIGTERM')
    await portForwardCompletion

    return {
      artifactPath,
      artifactSha256: downloadedSha256,
      evaluatorOutput: evaluation.output,
      podName,
      runIds,
      score,
    }
  }
  finally {
    portForward?.kill('SIGTERM')
    if (profilesFile != null) {
      await unlink(profilesFile).catch(() => undefined)
      await unlink(`${profilesFile}.lock`).catch(() => undefined)
    }
    if (podCreated && process.env.OSWORLD_KEEP_POD !== 'true') {
      const deleted = await runLoggedCommand('kubectl', [
        '--kubeconfig',
        kubeconfig,
        '--namespace',
        item.infrastructure.namespace,
        'delete',
        `pod/${podName}`,
        '--ignore-not-found=true',
        '--wait=true',
        '--timeout=2m',
      ], { timeout: 130_000 }, { caseId: item.id, podName, runtime: 'host', tool: 'kubectl' })
      recordKubernetesResource({
        action: 'delete',
        apiVersion: 'v1',
        caseId: item.id,
        exitCode: deleted.exitCode,
        kind: 'Pod',
        name: podName,
        namespace: item.infrastructure.namespace,
        outcome: deleted.exitCode === 0 ? 'succeeded' : 'failed',
      })
    }
  }
}

function assertExecuteSucceeded(result: ExecuteResponse, action: string): void {
  if (result.status !== 'success' || result.returncode !== 0) {
    throw new Error(`Failed to ${action}: ${result.error || result.output}`)
  }
}

async function checked(
  command: string,
  args: string[],
  options: { signal?: AbortSignal, stdin?: string, timeout: number },
  context: { caseId: string, podName: string, tool: 'auv' | 'kubectl' },
): Promise<void> {
  const result = await runLoggedCommand(command, args, {
    signal: options.signal,
    stdin: options.stdin,
    timeout: options.timeout,
  }, { ...context, runtime: 'host' })
  if (result.exitCode !== 0) {
    throw new Error(`${command} ${args.join(' ')} failed (${String(result.exitCode)}): ${result.stderr || result.stdout}`)
  }
}

async function connectSetup(
  kubectl: string[],
  podName: string,
  localPort: number,
  localAuvPort: number,
  endpoint: string,
  caseId: string,
  signal: AbortSignal,
): Promise<{ completion: Promise<void>, process: ReturnType<typeof startLoggedCommand>['process'] }> {
  for (let forwardAttempt = 0; forwardAttempt < 120; forwardAttempt += 1) {
    const activeCommand = startLoggedCommand('kubectl', [
      ...kubectl,
      'port-forward',
      `pod/${podName}`,
      `${localPort}:5000`,
      `${localAuvPort}:8080`,
    ], { signal }, { caseId, podName, runtime: 'host', tool: 'kubectl' })
    const process = activeCommand.process
    let ended = false
    let exitCode: number | undefined
    const completion = activeCommand.completion.then((output) => {
      ended = true
      exitCode = output.exitCode
    })

    for (let probeAttempt = 0; probeAttempt < 4; probeAttempt += 1) {
      await delay(1_000, signal)
      if (ended) {
        break
      }
      try {
        const response = await fetch(`${endpoint}/screenshot`, { signal })
        if (response.ok) {
          return { completion, process }
        }
      }
      catch {
        if (signal.aborted) {
          throw signal.reason
        }
      }
    }

    process.kill('SIGTERM')
    await completion
    if (exitCode !== 0 && forwardAttempt === 119) {
      throw new Error(`kubectl port-forward could not reach the OSWorld setup API (last exit ${String(exitCode)}).`)
    }
  }
  throw new Error('OSWorld setup API did not become ready within eight minutes.')
}

async function delay(milliseconds: number, signal: AbortSignal): Promise<void> {
  await new Promise<void>((resolveDelay, reject) => {
    const timeout = setTimeout(resolveDelay, milliseconds)
    signal.addEventListener('abort', () => {
      clearTimeout(timeout)
      reject(signal.reason)
    }, { once: true })
  })
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

async function installAuv(
  endpoint: string,
  binaryPath: string,
  binaryBytes: Uint8Array,
  expectedSha256: string,
  caseId: string,
  podName: string,
  signal: AbortSignal,
): Promise<void> {
  const form = new FormData()
  form.append('file_path', '/home/user/auv')
  form.append('file_data', new Blob([new Uint8Array(binaryBytes).buffer]), basename(binaryPath))
  const upload = await fetch(`${endpoint}/setup/upload`, { body: form, method: 'POST', signal })
  if (!upload.ok) {
    throw new Error(`OSWorld rejected the AUV upload with HTTP ${upload.status}: ${await upload.text()}`)
  }

  const packages = await installRuntimeLibraries(async () => await execute(endpoint, {
    command: 'echo password | sudo -S apt-get update && echo password | sudo -S apt-get install -y --no-install-recommends libtesseract4 liblept5 tesseract-ocr-eng',
    shell: true,
  }, signal), signal, { caseId, podName })
  assertExecuteSucceeded(packages, 'install AUV runtime libraries')

  const launchCommand = `chmod 0755 /home/user/auv && test "$(sha256sum /home/user/auv | cut -d' ' -f1)" = "${expectedSha256}" && rm -f /home/user/auv.sock && nohup env DISPLAY=:0 XDG_SESSION_TYPE=x11 /home/user/auv serve --listen unix:///home/user/auv.sock --listen http://0.0.0.0:8080 --pairing-store /home/user/.local/share/auv-osworld/pairings.json --store-root /home/user/.local/share/auv-osworld --no-register </dev/null >/tmp/auv.log 2>&1 &`
  const launch = await observeGuestCommand('/bin/sh', ['-c', launchCommand], {
    caseId,
    podName,
    tool: 'auv',
  }, async () => await execute(endpoint, {
    command: launchCommand,
    shell: true,
  }, signal), result => result.returncode)
  assertExecuteSucceeded(launch, 'launch the guest-local AUV daemon')

  for (let attempt = 0; attempt < 60; attempt += 1) {
    const probe = await execute(endpoint, { command: ['test', '-S', '/home/user/auv.sock'] }, signal)
    if (probe.returncode === 0) {
      return
    }
    await delay(1_000, signal)
  }
  throw new Error('Guest-local AUV daemon did not create its Unix socket within 60 seconds.')
}

/**
 * Normalizes the three placeholders used by the checked-in OSWorld setup.
 *
 * Before:
 * - `echo {CLIENT_PASSWORD}`
 * - `click({SCREEN_WIDTH_HALF}, {SCREEN_HEIGHT_HALF})`
 *
 * After:
 * - `echo password`
 * - `click(960, 540)`
 */
function interpolateSetupValue(value: string): string {
  return value
    .replaceAll('{CLIENT_PASSWORD}', 'password')
    .replaceAll('{SCREEN_WIDTH_HALF}', '960')
    .replaceAll('{SCREEN_HEIGHT_HALF}', '540')
}

async function invokeAuv(
  paired: PairedAuv,
  command: string,
  args: string[],
  caseId: string,
  podName: string,
  signal: AbortSignal,
): Promise<InvokeResponse> {
  const result = await runLoggedCommand(paired.binary, [
    '--device-id',
    paired.deviceId,
    'invoke',
    command,
    ...args,
    '--store-root',
    paired.storeRoot,
    '--no-overlay',
    '--json',
  ], {
    nodeOptions: {
      env: {
        ...process.env,
        AUV_CONFIG_PROFILES_FILE: paired.profilesFile,
      },
    },
    signal,
    timeout: 120_000,
  }, { caseId, podName, runtime: 'host', tool: 'auv' })
  if (result.exitCode !== 0) {
    throw new Error(`AUV command ${command} failed (${String(result.exitCode)}): ${result.stderr || result.stdout}`)
  }
  const parsed: unknown = JSON.parse(result.stdout)
  if (!isRecord(parsed) || typeof parsed.run_id !== 'string' || parsed.status !== 'completed') {
    throw new Error(`AUV command ${command} returned an unexpected JSON result: ${result.stdout}`)
  }
  const artifacts = Array.isArray(parsed.artifacts)
    ? parsed.artifacts.filter((artifact): artifact is { file_path: string, sha256: string } => {
        return isRecord(artifact) && typeof artifact.file_path === 'string' && typeof artifact.sha256 === 'string'
      })
    : []
  return {
    artifacts,
    run_id: parsed.run_id,
    status: parsed.status,
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

async function pairAuv(
  setupEndpoint: string,
  binary: string,
  localAuvPort: number,
  evidenceDirectory: string,
  caseId: string,
  podName: string,
  signal: AbortSignal,
): Promise<PairedAuv> {
  const tokenCommand = [
    'env',
    'AUV_ENDPOINT=unix:///home/user/auv.sock',
    '/home/user/auv',
    'devices',
    'pair',
    'create-token',
  ]
  const tokenResult = await observeGuestCommand(tokenCommand[0], tokenCommand.slice(1), {
    caseId,
    podName,
    tool: 'auv',
  }, async () => await execute(setupEndpoint, {
    command: [
      ...tokenCommand,
    ],
  }, signal), result => result.returncode)
  assertExecuteSucceeded(tokenResult, 'create the AUV pairing token')
  const token = tokenResult.output.trim()
  if (!/^[a-f0-9]{32}$/.test(token)) {
    throw new Error('Guest AUV returned an invalid pairing token.')
  }

  const profilesFile = join(evidenceDirectory, 'device-profiles.json')
  const storeRoot = join(evidenceDirectory, 'auv-runs')
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
    nodeOptions: {
      env: {
        ...process.env,
        AUV_CONFIG_PROFILES_FILE: profilesFile,
      },
    },
    signal,
    stdin: token,
    timeout: 60_000,
  }, { caseId, podName, runtime: 'host', tool: 'auv' })
  if (connected.exitCode !== 0) {
    throw new Error(`AUV pairing failed (${String(connected.exitCode)}): ${connected.stderr || connected.stdout}`)
  }
  const enrollment: unknown = JSON.parse(connected.stdout)
  if (!isRecord(enrollment) || typeof enrollment.device_id !== 'string') {
    throw new Error(`AUV pairing returned an unexpected response: ${connected.stdout}`)
  }
  return {
    binary,
    deviceId: enrollment.device_id,
    profilesFile,
    storeRoot,
  }
}

function podManifest(item: OsworldCase, podName: string): Record<string, unknown> {
  return {
    apiVersion: 'v1',
    kind: 'Pod',
    metadata: {
      labels: {
        'app.kubernetes.io/managed-by': 'vieval',
        'app.kubernetes.io/name': 'osworld-episode',
        'vieval.dev/case': item.id,
      },
      name: podName,
      namespace: item.infrastructure.namespace,
    },
    spec: {
      containers: [{
        env: [
          { name: 'DISK_SIZE', value: '32G' },
          { name: 'RAM_SIZE', value: '8G' },
          { name: 'CPU_CORES', value: '4' },
          // NOTICE: The OSWorld image's dnsmasq can exhaust file descriptors on
          // this node. QEMU user-mode forwarding keeps the setup API reachable
          // without changing node sysctls. Remove when the runtime image fixes it.
          { name: 'USER_PORTS', value: '5000,8080' },
        ],
        image: item.infrastructure.runtimeImage,
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
        'feature.node.kubernetes.io/kvm': 'true',
        'kubernetes.io/arch': 'amd64',
        // NOTICE: KVM availability does not prove that the pinned OSWorld
        // guest reaches its setup API. Operators add this label only after a
        // live episode probe; adding automatic qualification is deferred until
        // the cluster has a reusable node-conformance controller.
        'auv.moeru.ai/osworld-ready': 'true',
      },
      restartPolicy: 'Never',
      terminationGracePeriodSeconds: 30,
      volumes: [
        { name: 'image', persistentVolumeClaim: { claimName: item.infrastructure.basePvc, readOnly: true } },
        { hostPath: { path: '/dev/kvm', type: 'CharDevice' }, name: 'kvm' },
      ],
    },
  }
}

async function prepareTask(endpoint: string, item: OsworldCase, signal: AbortSignal): Promise<void> {
  for (const step of item.task.config) {
    const command = Array.isArray(step.parameters.command)
      ? step.parameters.command.map(interpolateSetupValue)
      : interpolateSetupValue(step.parameters.command)
    const result = await execute(endpoint, { command, shell: step.parameters.shell }, signal)
    assertExecuteSucceeded(result, `prepare OSWorld task ${item.task.id}`)
  }
}

function requiredEnvironment(name: string): string {
  const value = process.env[name]
  if (value == null || value.length === 0) {
    throw new Error(`${name} must name the local input required for an OSWorld run.`)
  }
  return value
}
