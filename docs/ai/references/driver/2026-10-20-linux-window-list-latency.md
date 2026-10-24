# Linux `window.list` latency: the GNOME Shell stage walk

Status: implemented 2026-10-20. Scope: `crates/auv-driver-linux/src/atspi.rs`
only. No capability or support-matrix change.

## Problem

`auv invoke window.list --json` took 8 to 12 seconds on GNOME Wayland. Every
window-targeted command pays that cost because the window list is the first
step: `window.capture` resolves the window and then lists before and after the
capture (three lists), and `window.waitForText` / `window.scrollUntil` list once
per poll attempt, so the poll exceeded the default 10 s command timeout before
it could take a second look.

## Root cause

Two costs were stacked in `apply_shell_stage_window_origins` /
`shell_stage_rects`:

1. The stage walk ran unconditionally, before checking whether any listed
   window needed its origin repaired.
2. The walk covered the whole GNOME Shell "Main stage" subtree with the generic
   `walk`, which reads role, name, description, accessible id, state, both
   coordinate-space extents, attributes, and children for every node.

Measured on 2026-10-20, GNOME Shell 50.4 Wayland with PaperWM (2752×1152), debug
build (`gnome-shell --version`):

| metric | before |
| --- | --- |
| `window.list` wall time (3 runs, earlier sample) | 9.24 s / 8.22 s / 9.92 s |
| a11y method calls per run | 17,289 |
| nodes visited | 2,000 (`MAX_NODES` cap) |
| `GetRoleName` / `GetExtents` / `GetAttributes` | 2,003 / 4,003 / 2,000 |
| `GetAll` + `AddMatch` + `RemoveMatch` (property reads) | 2,011 each |

The repair itself is required, not a fallback that rarely runs. The listed
Ghostty window reports `ATSPI_COORD_TYPE_SCREEN` extents `(0, 0, 800, 1077)`
because GTK4 on Wayland does not know its global position, and
`window.list`'s JSON shows the *repaired* origin. The stage tree is the only
source for that origin on GNOME Wayland: `org.gnome.Shell.Introspect.GetWindows`
returns `AccessDenied` to ordinary clients, and the window's children repeat the
same `(0, 0)` extents.

## Fix

1. `stage_repair_frames` collects the frames that actually need repair
   (non-shell app window, origin `(0, 0)`, non-empty area). An empty list
   returns before any walk. This is the fast path for toolkits that report real
   origins.
2. When a repair is needed, walk the shell's two window containers instead of the
   whole stage: "Window group" and "Top window group" (override-redirect
   windows such as popups and menus). `find_window_groups` does a breadth-first
   search with a 3-level depth limit and a 200-node budget, so it normally
   costs a couple of dozen name reads. The scoped walk runs only when both
   containers are found; a miss, a read error, an exhausted search budget, or a
   shell that exposes one container falls back to the whole-stage walk, because
   a partial candidate set can change the uniqueness result.
3. `shell_stage_rects` keeps only rects whose size matches a frame under repair.
   `matching_stage_origin` already ignored every other rect, so this is
   semantically neutral and only shrinks the candidate vector.

The container names come from gnome-shell's window manager: `libshell-18.so`
contains `global.window_group` and `global.top_window_group`, and the measured
session exposed both under the shell's stage container with those names.
`Top window group` is empty on a normal desktop and fills with
override-redirect windows, so including it costs one node while keeping those
windows in the candidate set and the uniqueness check conservative.

## Result

Same desktop, same command, after the fix:

| metric | before | after |
| --- | --- | --- |
| nodes visited | 2,000 | 64 (63 + empty top group) |
| a11y method calls per run | 17,289 | 683 |
| `window.list` wall time | 2.4–2.9 s at load ~2; 8–12 s at load 6.5 | 0.09–0.25 s at load ~2; 0.33–0.54 s at load 6.5 |

The listed window still receives a real origin (e.g. `5192, 55, 800×1077`), so
the repair output is unchanged in kind. The wall-time spread tracks machine
load; the stable metric is the ~25× drop in a11y traffic.

## Ablation

Six binaries were built from the same source with the gate and the window-group
scope toggled. `v4`/`v5` force `needs_shell_stage_origin` to `false` to emulate a
desktop whose windows report real origins; no such window exists on this host,
so those two rows are synthetic. Wall times are medians of three interleaved
runs; call counts come from one monitored run each. The run window had a load
around 2, so absolute times are lower than in the first measurement above;
compare call counts first.

| variant | gate | stage walk | repair needed | a11y calls | median |
| --- | --- | --- | --- | --- | --- |
| v0 baseline | no | whole stage | yes | 17,289 | 2.4 s |
| v1 gate only | yes | whole stage | yes | 17,289 | 2.6 s |
| v2 group only | no | window containers | yes | 683 | 0.14 s |
| v3 both (shipped) | yes | window containers | yes | 683 | 0.13 s |
| v4 no repair, no gate | no | whole stage | no | 17,289 | 2.6 s |
| v5 no repair, gate | yes | whole stage | no | 40 | 0.02 s |

- The window-container scope is the win whenever a repair is needed: 17,289 → 683.
- The gate is a no-op in that state (v1 ≈ v0) and is what makes the no-repair
  state cheap (v4 → v5). Both halves are load-bearing: GTK4 Wayland windows hit
  the repair path, toolkits that report real origins hit the gate.
- v2 and v3 perform identical work; their wall-time difference is load noise.
- The size filter runs on the already-collected nodes after `walk`, so it cannot
  change D-Bus traffic; it only keeps the candidate vector small.

## Boundaries

- The repair path still uses the generic `walk` (about 8.6 D-Bus calls per node)
  even though it only needs bounds and child counts. See
  `TODO(linux-shell-stage-walk)` at the call site: a bounds-only traversal must
  reproduce `accessible_in_tree`'s WINDOW-then-SCREEN extents exactly and keep
  the uniqueness check.
- Both container names are observed on GNOME Shell 50.4. Discovery is
  all-or-nothing: a miss, a read error, a search budget exhaustion, or a shell
  with one container falls back to the whole-stage walk; re-check the names on
  GNOME Shell major updates. The driver has no logging channel, so a discovery
  failure is not reported separately from a missing container.
- The uniqueness rule is unchanged: two stage rects of the repaired size with
  different origins still produce no repair. The candidate set now covers both
  window containers; shell chrome (top bar, labels, panel items) stays excluded.
  Chrome is not a window, but a same-size chrome rect could previously block a
  repair that now succeeds; the matched rect is always a window actor of the
  same size.
- No support claim changed. `docs/SUPPORT_MATRIX.md` keeps Linux at `not
  claimed` for capture, OCR, AX, and input.

## Reproduction

```sh
# Call-count and latency evidence. Keep the monitor short-lived: an
# eavesdropping dbus-monitor measurably slows every AT-SPI reply.
timeout 20 dbus-monitor --address "unix:path=/run/user/1000/at-spi/bus" \
  > /tmp/a11y.log 2>&1 &
monitor=$!
sleep 1
start=$(date +%s%N)
target/debug/auv invoke window.list --json >/dev/null
echo "$(( ($(date +%s%N) - start) / 1000000 )) ms"
kill "$monitor"
grep -c 'method call' /tmp/a11y.log
```

`cargo test -p auv-driver-linux --lib` covers `stage_repair_frames` (skip and
collect cases) plus the existing `matching_stage_origin` cases.
