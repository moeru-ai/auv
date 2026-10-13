# Windows background scroll and `scroll_until` on QQ Music

Status: evidence note for the Windows scroll work in #348
([#211](https://github.com/moeru-ai/auv/issues/211)). It replaces the six slice documents that
#348 merged at the repository root (`settle-timing.md`, `scroll-until-qqmusic-textvisible.md`,
`scroll-until-end.md`, `scroll-until-motion.md`, `wheel-acceptance-matrix.md` and
`windows-211-scroll-parity-completion.md`, recoverable at `ed81787a`). It keeps what the records
support and corrects what they do not. The
[consolidation review](../invoke-cli/2026-10-13-windows-parity-consolidation-review.md) has the
full reasoning.

No `support` cell changed. `input.scroll` and `input.scrollUntil` stay
`Partial("background delivery posts window messages, so whether an application acts on them
depends on its toolkit")` on Windows (`crates/auv-cli-invoke/src/commands/input.rs`). That note
is about the source path; which applications act on a posted wheel is a behavior question, and
its answers belong in the
[support matrix](../../../SUPPORT_MATRIX.md#windows-background-session-by-application).

## Setup

- One Windows 11 machine, on 2026-10-12.
- The target was the user's own QQ Music window, 1280x822, while a track played. Its lyrics view
  and its playlist view were scrolled.
- Every `scroll_until` run went through `auv invoke input.scrollUntil` with
  `--input-policy background-only`, so each step was a posted `WM_MOUSEWHEEL`
  (`window_targeted_wheel`), with `--settle-ms 50`.
- No independent witness ran (the evals' `tasks/watch.rs`). That the foreground, the cursor and
  the window state were left alone rests on the input policy, not on an observation.

Records (window titles, lyrics and OCR text are redacted; the redaction keeps run IDs, steps,
deltas, stop reasons and motion figures):
[`evidence/2026-10-12-windows-scroll-until-background/`](evidence/2026-10-12-windows-scroll-until-background/).
The tables below shorten run IDs to their first two groups.

## Results

### Text visible: 4 of 4, and a clean budget stop

`--until text:<query>` with `--region 0.4,0.2,0.5,0.6` around the lyrics pane
([`text-visible.jsonl`](evidence/2026-10-12-windows-scroll-until-background/text-visible.jsonl)):

| Run | Steps | Delivered `dy` | Stop reason | Checked by |
| --- | --- | --- | --- | --- |
| `01a12fd6-e0ee` | 4 | 480 | `text_visible` | A follow-up `window.findText` returned the same bounds |
| `01a12fd7-5f81` | 3 | 360 | `text_visible` | The same |
| `01a12fd8-2325` | 1 | 120 | `text_visible` | OCR of the step's capture |
| `01a12fd8-c0a9` | 4 | 480 | `text_visible` | A follow-up `window.findText` returned the same bounds |
| `01a12fd9-317b` | 10 (`--max-steps 10`) | 1200 | `budget_exhausted` | Query text that is not on the page |

The original document says each query was off screen before its run (`window.findText` found
nothing); the records do not hold that check.

### End: head and tail of the lyrics

`--until end` ([`end.jsonl`](evidence/2026-10-12-windows-scroll-until-background/end.jsonl)):

| Run | Region | Steps | Delivered `dy` | Stop reason | Last step's diff |
| --- | --- | --- | --- | --- | --- |
| `01a12fde-4f20`, tail | `0.4,0.2,0.5,0.6` | 48 | +5760 | `end_by_no_visual_progress` | 0.0084 |
| `01a12feb-37f3`, head | `0.52,0.24,0.22,0.50` | 34 | −4080 | `end_by_no_visual_progress` | 0.0004 |
| `01a12fea-51a1`, `01a12fea-c488` | `0.52,0.24,0.22,0.50` | 50 | −6000 | `budget_exhausted` | 0.0094, 0.0095, still moving (shift 20) |
| `01a12fe9-d357` | `0.4,0.2,0.5,0.6` | 2 | −240 | `end_by_no_visual_progress`, too early | 0.0098 |

The final captures of the head and tail runs show the first and last lyric lines with nothing
beyond them.

### Timed motion: four runs moved the playlist

`--step-duration-ms` with four timing functions, one step each, on the playlist view with
`--region 0.20,0.30,0.70,0.65`
([`motion.jsonl`](evidence/2026-10-12-windows-scroll-until-background/motion.jsonl)):

| Run | Motion | Requested → delivered `dy` | Wheel units | Diff reported by `auv` | Whole command |
| --- | --- | --- | --- | --- | --- |
| `01a13001-dc63` | 300 ms linear | 120 → 120 | 144 | 0.037 | 1644 ms |
| `01a13001-edbe` | 500 ms ease-out | 240 → 240 | 288 | 0.032 | 1800 ms |
| `01a13002-00d7` | 400 ms ease-in-out | −120 → −120 | −144 | 0.040 | 1802 ms |
| `01a13002-13b3` | 350 ms ease-in | 121 → 120.83 | 145 | 0.0093 | 1610 ms |

- The delivered totals are the cumulative quantization in 1/120-notch units, which
  `auv-driver-common/src/scroll_motion_test.rs` already tests.
- What these runs add is that the frames moved. The fourth run's diff as `auv` reported it,
  0.0093, is under the 0.01 no-motion threshold. Its record also holds a second figure, 0.038
  (`motion_diff_calculated`), whose computation is not recorded.
- The per-tick times in the records are the planned schedule (`scheduled_elapsed_ms`). Nothing
  measured when the samples were dispatched, so these runs do not show that the timing matched the
  requested duration. "Whole command" is the wall time of the `auv` process.

### Settle bench: one notch reaches a frame in tens of milliseconds

The `measure-settle` driver bench (`crates/auv-driver-windows/src/bin/measure_settle.rs`, not
`auv invoke`). It posted one notch (±120) at a time to QQ Music and to an Explorer window showing
the Downloads folder, and read the window's WGC frame stream
([`settle-timing.jsonl`](evidence/2026-10-12-windows-scroll-until-background/settle-timing.jsonl),
recomputed from the 110 records):

| Surface | Samples | First changed frame, P50 / P95 | Last moving frame, P50 / P95 / max |
| --- | --- | --- | --- |
| QQ Music, warm | 54 | 12.5 / 14.2 ms | 28.4 / 30.4 / 31.2 ms |
| Explorer, warm | 54 | 32.7 / 36.0 ms | 32.7 / 37.8 / 45.4 ms |
| QQ Music, cold | 1 | 12.9 ms | 29.4 ms |
| Explorer, cold | 1 | 41.2 ms | 89.2 ms |

None of the 110 stalled. Before the bench, `mediaControl.pause` paused the user's music so that
playback would not change the frames.

**The `scroll_until` settle default stays 400 ms** (`auv_scan::ScrollUntilRequest::DEFAULT_SETTLE`):

- The bench measured how fast one notch reaches a frame on content that was already loaded.
- The default exists for another reason. The command waits "so lazy content can load"
  (`input.scrollUntil --help`), and no loading time was measured.
- Each "cold" figure is one sample.
- A caller that knows its surface can pass `--settle-ms 50`, as these runs did.

Changing the default per platform would need a measurement on a lazy-loading list and owner
approval.

## Findings

- **Multi-pane windows need `--region`.** Lyrics take about a quarter of the QQ Music window.
  Compared over the whole window, a step that moved them changed 0.73% of the pixels, under the
  1% no-motion threshold. Two such steps ended the loop with `end_by_no_visual_progress` before
  the target (`01a12fd9-6428` in `text-visible.jsonl`). Over the lyrics pane the same steps
  changed 2.7 to 2.9%. A region that is still too broad fails the same way: `01a12fe9-d357` ended
  after 2 steps at 0.98%.
- **An application that ignores the wheel ends the loop the same way as real end of content.**
  One `--until end` run on the user's Steam friends list stopped after 2 steps with a diff of
  exactly 0, under `end_by_no_visual_progress`. `scroll_until` cannot tell "never moved" from
  "stopped moving"; the [scroll until](../../../TERMS_AND_CONCEPTS.md#scroll-until) term already
  says an end is an observation, not proof.
- **Windows OCR puts a space between CJK ideographs**, so a Chinese query of several characters
  without spaces does not match. Single characters, or words written with the spaces, matched.
- **QQ Music scrolls its lyrics back to the playing line** a few seconds after the last scroll. A
  check made after a run has to come straight after it.

## Applications that showed no motion

The same day, one `--until end` run of 2 steps was made on each of these windows, all of them
applications the user had open: Edge Beta, Claude Desktop, NTQQ, VirtualBox Manager and the Steam
friends list
([`wheel-by-app.jsonl`](evidence/2026-10-12-windows-scroll-until-background/wheel-by-app.jsonl)).
None of the frames changed.

These runs cannot carry a claim:

- **There was no positive control.** No run sent the same wheel at the same point with
  `foreground-preferred`.
- **The windows' state was not recorded.** It is not known whether they were covered. A covered
  Chromium window stops painting, so its frames show no change whatever happens to the page
  ([background session](2026-10-10-windows-background-session.md#applications)). VirtualBox's
  manager list may simply have had nothing to scroll.
- **The mechanism was not observed.** The slice documents said Chromium, Electron, CEF and Qt drop
  posted wheels unless they have the focus or the hover, by design, for every derivative. The
  records show none of that. The open question from #335 is a different mechanism: Chromium may
  redirect a wheel to whichever window is under the point (`RerouteMouseWheel`). Posted clicks do
  reach a Chromium page (#335: 3 of 4 with default flags, 4 of 4 with occlusion tracking off), so
  "Chromium drops posted input" is not a rule.
- **Toolkit labels are unverified.** No record holds a window class name. The slice documents
  called QQ Music both "DirectComposition / CEF" and a "Win32 custom container".

The record file also holds two summary rows that are not runs. The QQ Music row repeats a
text-visible result under its own run ID (`01a12fd6-80f0`). The Explorer row's run ID
(`brief2-explorer-benchmark-01`) and its delta (100) appear in no bench record.

The by-application table in the support matrix gains rows only from a controlled rerun:

- the evals' `apps` scenario;
- applications the harness launched itself;
- a foreground control at the same point;
- occlusion tracking off for Chromium.

## The user's desktop

These runs scrolled applications the user had open, and paused their music. No record says
whether the owner allowed that. From now on a Windows eval or probe acts only on windows it
created or launched itself (`evals/auv-base/platforms/desktop-windows/README.md`). The
`measure-settle` bench still targets QQ Music and Explorer;
`TODO(windows-settle-receiver)` (#353) marks the switch to a receiver it owns.
