# Run record: 2026-10-11, PR 258 AUV `a2c8ce64`

Four Vieval batches, 14 OSWorld episodes, 6 tasks. Each episode used one
fresh KVM Pod. Every episode reached the upstream evaluator. No episode had a
harness error.

**Result:** 12 of 14 episodes scored 1 (85.7 %). 4 of 6 tasks have pass@k = 1.

| Task | Instruction (short) | Attempts | Scores | Mean |
| --- | --- | ---: | --- | ---: |
| os/e0df059f | Rename `todo_list_Jan_1` to `todo_list_Jan_2` | 5 | 1, 1, 1, 1, 1 | 1.00 |
| os/28cc3b7e | Set the system volume to the maximum | 4 | 1, 1, 1, 1 | 1.00 |
| os/bedcedc4 | Set "Dim screen when inactive" to off | 2 | 1, 1 | 1.00 |
| os/b6781586 | Set the time zone to UTC+0 (see the note below) | 1 | 1 | 1.00 |
| vs_code/982d12a5 | Change the VS Code color theme to Visual Studio Dark | 1 | 0 | 0.00 |
| os/ec4e3f68 | Remove Vim from the favorite apps | 1 | 0 | 0.00 |

`../../summary.md` and `../../summary.json` hold the per-episode rows. The
JSON also holds every model reply and every action.

## Environment

| Item | Value |
| --- | --- |
| Source | `moeru-ai/auv` branch `codex/pr-258-rebased`. It is rebased onto `origin/main` `d0435522`. The run used commit `53e3b2fd` plus the two small changes in this record. |
| Guest AUV | `auv-linux-x86_64`, built by `scripts/build-auv-linux` from `a2c8ce64` (Ubuntu 22.04 glibc). sha256 `71318ffa3ce484d222e65c44bf83c1f0fc6152d787558384a87da5f6a3e72982` |
| Host AUV | `auv-macos-arm64` from the same commit `a2c8ce64`, `auv 0.0.32`. sha256 `aeb078f0b1ea149f62b3cff084e0edcc6c6e6cfade860ffead8d77ac6bc35084` |
| Source drift | `a2c8ce64..53e3b2fd` changes only `evals/osworld/scripts/install-auv`. No Rust source changed, so both binaries match the source of the commit that ran. |
| Cluster | `KUBECONFIG=~/.kube/config.d/ihome.conf`, context `neko-mbp14@kubernetes`, server v1.36.3, namespace `benchmark-osworld-auv`, node `liet-gpu-1` (KVM, label `auv.moeru.ai/osworld-ready=true`) |
| VM | Pinned `happysixd/osworld-docker@sha256:0e64…aa9`, base PVC `osworld-v1-base` (read-only `System.qcow2`). Each guest has 8 GiB RAM, 4 vCPU, and a 1920x1080 screen. |
| OSWorld | Upstream `xlang-ai/OSWorld` `b138d348`. The checkout is `~/.cache/auv-osworld/OSWorld` (`tasks/REVISION`). The Pixi env `osworld` holds the evaluator dependencies. |
| Model | `gpt-5.5` through the local CLIProxyAPI 7.2.145 at `http://127.0.0.1:8317/v1` (Codex OAuth accounts, no API key) |
| Harness | Node v25.9.0, pnpm 11.9.0, vieval 0.0.12, tinyexec 1.3.0, `OSWORLD_MAX_STEPS=15` |

## Commands

From `evals/osworld/`:

```bash
export OSWORLD_AUV_LINUX_BINARY=/tmp/auv-pr258-a2c8ce64/auv-linux-x86_64
export OSWORLD_AUV_MACOS_BINARY=/tmp/auv-pr258-a2c8ce64/auv-macos-arm64
export OSWORLD_KUBECONFIG=$HOME/.kube/config.d/ihome.conf
export OSWORLD_SOURCE=$HOME/.cache/auv-osworld/OSWorld
export OSWORLD_MODEL=gpt-5.5

pnpm -s doctor                                    # 11/11 checks passed -> doctor.json

# Batch 1: 2 tasks x 3 repeats, 2 Pods in parallel (15 min 25 s)
OSWORLD_TASK_IDS=e0df059f-28a6-4169-924f-b9623e7184cc,28cc3b7e-b194-4bc9-8353-d04c0f4d56d2 \
OSWORLD_REPEAT=3 OSWORLD_CONCURRENCY=2 pnpm -s eval:run    # vieval-run-1.log: cases 6 passed | 0 failed

# Batch 2: 3 harder tasks, 3 Pods in parallel (8 min 42 s)
OSWORLD_TASK_IDS=982d12a5-beab-424f-8d38-d2a48429e511,bedcedc4-4d72-425e-ad62-21960b11fe0d,b6781586-6346-41cd-935a-a6b1487918fc \
OSWORLD_REPEAT=1 OSWORLD_CONCURRENCY=3 pnpm -s eval:run    # vieval-run-2.log: cases 2 passed | 1 failed

# Batch 3: after the evaluator-stdout change (7 min 8 s)
OSWORLD_TASK_IDS=e0df059f-28a6-4169-924f-b9623e7184cc,ec4e3f68-9ea4-4c18-a5c9-69f89d1178b3 \
OSWORLD_REPEAT=1 OSWORLD_CONCURRENCY=2 pnpm -s eval:run    # vieval-run-3.log: cases 1 passed | 1 failed

# Batch 4: confirmation after the model outage, at commit 1347d6b7 (3 min 34 s)
pnpm -s doctor                                    # 11/11 checks passed -> doctor-run-4.txt
OSWORLD_TASK_IDS=e0df059f-28a6-4169-924f-b9623e7184cc,28cc3b7e-b194-4bc9-8353-d04c0f4d56d2,bedcedc4-4d72-425e-ad62-21960b11fe0d \
OSWORLD_REPEAT=1 OSWORLD_CONCURRENCY=3 pnpm -s eval:run    # vieval-run-4.log: cases 3 passed | 0 failed

pnpm -s results:summarize                         # 6 task/model rows from 14 episodes
```

The Vieval exit status is 0 even when a case fails. Read the `cases N passed
| M failed` line or `summary.md` for the result.

## Chain continuity: what each link shows

| Link | Evidence in this directory | Status |
| --- | --- | --- |
| Linux x64 AUV from PR 258 | Every episode uploads the binary through the OSWorld `/setup/upload` API. The guest then runs `test "$(sha256sum /home/user/auv …)" = "71318ffa…"` before `auv serve` (`vieval-run-*.log`, `tool=auv runtime=guest`). `result.json` repeats `auvLinuxSha256`. | Verified, 11/11 |
| Pod | `kubectl create` of `osworld-<task>-<rand>` with the pinned image and the read-only base PVC. `kubectl wait` for Running, `port-forward` for 5000/9222/8080/9847, and a UID-scoped delete (`osworld:kubernetes-resource` lines). After the run, `kubectl get pods` returned `No resources found`. | Verified, 11 created, 11 deleted |
| Pair token | In each fresh VM, the guest runs `auv devices pair create-token`. The host runs `auv devices pair … connect --token-stdin` (`hasStdin=true`), so the token never appears in a log. All later `invoke` calls in that episode use the one returned `--device-id`. Example from batch 1: `62bea535…` 34 log lines, `44c12d4f…` 30, `4d582869…` 18. One VM has one device for its whole life. | Verified |
| Pair token across reconnects | This is not exercised here, because each VM pairs once and is then deleted. The T-95 run with `scripts/install-auv` showed `paired_now:false` and the same `device_id`, `pair_id`, and credential sha256 after a new tunnel and after a guest-daemon restart. | Covered by T-95, not repeated |
| Model endpoint | `doctor.json`: `/models` lists `gpt-5.5`, and text and image completions returned `OK`. `episodes/*/result.json` `agent.steps[].reply` holds the raw replies from 103 model turns. The replies are non-deterministic and often concatenate several planned turns, so they are not a stub. CLIProxyAPI wrote no error log from 17:55Z to 18:37Z. | Verified |
| Task setup | `episodes/*/bridge-setup.log`: the upstream `SetupController` steps, for example `SETUP COMPLETED: _execute_setup(... mkdir ~/Desktop/todo_list_Jan_1 ...)` | Verified, 11/11 |
| Scoring | `episodes/*/bridge-evaluate.log` holds the upstream getter output, for example `['thunderbird.desktop', 'vim.desktop', 'google-chrome.desktop']` for ec4e3f68. In batch 3 the log ends with the bridge verdict (`--- stdout (exit 0) --- {"score": 0.0}`). Vieval records the same number with `context.score` and asserts `score === 1`. | Verified |
| Artifacts | `.vieval/episodes/<task>/<time>-<model>-r<n>/` (local, gitignored), plus this committed copy: `result.json`, the bridge logs, and `auv-records.jsonl` (the AUV run-store index) per episode. | Verified |

Command outcomes across all four logs: 367 `succeeded`, 14 `stopped`
(port-forwards ended at cleanup), and 15 `failed`. All 15 failures are
`kubectl port-forward` exit 1 while QEMU is still booting.
`connectSetup` retries this by design. Every AUV, guest, and bridge command
succeeded.

## Failure samples

- **vs_code/982d12a5, score 0, 15 steps, no DONE.** The model opened VS Code
  and the command palette. Then it clicked the wrong palette row and opened
  Settings, dismissed popups, and pressed `Ctrl+K` and `Ctrl+T` as two
  separate turns. The second key opened symbol search (`#`). The step budget
  ran out. `settings.json` still had no `workbench.colorTheme`. See
  `samples/failure-982d12a5-final.webp`.
- **os/ec4e3f68, score 0, 15 steps, no DONE.** AUV right-click works:
  `samples/failure-ec4e3f68-step4-context-menu.webp` shows the Vim dock menu
  with "Remove from Favorites". The menu item is at about grid (89, 144), but
  the model clicked grid (178, 157), which is about 342x170 px and outside the
  menu. It repeated the same miss 5 times, typed `:qa!` into a Vim terminal,
  and then dragged the icon. The evaluator read
  `['thunderbird.desktop', 'vim.desktop', 'google-chrome.desktop']`. This is
  a coordinate-grid mistake by the model. See
  `samples/failure-ec4e3f68-final.webp`.
- **Pass sample:** `samples/pass-e0df059f-final.webp` shows `todo_list_Jan_2`
  on the desktop after `DONE`.
- **Terminal captures:** `samples/vieval-batches-terminal.png` (preflight,
  the three Vieval tails, the summary, no leftover Pods, and the post-run
  doctor) and `samples/chain-continuity-terminal.png` (one episode from Pod
  create to Pod delete).

## Caveats

- **os/b6781586 is trivially satisfied.** The upstream task has no setup step
  that moves the guest off UTC, and the base image is already UTC
  (`Time zone: UTC (UTC, +0000)` in `bridge-evaluate.log`). The model used all
  15 steps without `DONE`, and the score is still 1. Do not count this as
  agent capability.
- **e0df059f batch 3 scored 1 without DONE.** The rename happened early, and
  then the model kept acting until the step budget ran out. OSWorld scores the
  final state, so the score is valid. The step count is inflated.
- Each task has 1 to 4 attempts. The 81.8 % covers the 6 selected tasks. It is
  not a result for the OSWorld benchmark (361 tasks in `test_nogdrive`).
- **Model access ended after the run.** At 18:40:43Z, about 3 minutes after
  the last episode finished, CLIProxyAPI began to return
  `HTTP 503 auth_unavailable: no auth available (providers=codex, model=gpt-5.5)`.
  The doctor then failed only its two chat-completion checks, and 3 manual
  `curl` retries at 10-second intervals got the same 503. Only one Codex OAuth
  account is configured. The next run needs `cliproxyapi --codex-login`, or the
  account's quota window must reset. No episode in this record is affected:
  their 103 replies are in `result.json`, and the first 503 log file is
  `~/.cli-proxy-api/logs/error-v1-chat-completions-2026-10-11T034043-*.log`
  (local time +09:00). **Resolved at 18:49Z without a re-login:** the quota
  window reset, `doctor` passed 11/11 again (`doctor-run-4.txt`), and batch 4
  passed 3 of 3 (`samples/pass-e0df059f-run4-final.webp`).
- Episodes from 2026-10-10, with older binaries `8c9c2fde`, are kept locally
  in `.vieval/archive-2026-10-10/` and are not part of this summary.
