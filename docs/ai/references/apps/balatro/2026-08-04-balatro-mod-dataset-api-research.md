# Balatro Mod Dataset API Research

Date: 2026-08-04

## Question

What can AUV reuse conceptually from `coder/balatrollm`,
`FFFishes7/blinddeck`, `abhinavuppala/BalatroBotLearning`, and
`alesha-pro/evalatro` to inject a development-only Balatro mod and generate
ground-truth card-recognition training data?

This is a source review, not an implementation decision. All repositories were
cloned into a temporary directory outside the AUV worktree. No upstream code
was copied into AUV.

## Evidence snapshot

| Repository | Inspected commit | Declared license | Relevant role |
| --- | --- | --- | --- |
| [`coder/balatrollm`](https://github.com/coder/balatrollm/tree/6269c592ac4d4534934701f42773256342457b69) | `6269c592ac4d4534934701f42773256342457b69` | MIT | BalatroBot HTTP client, run loop, delayed screenshot collection |
| [`FFFishes7/blinddeck`](https://github.com/FFFishes7/blinddeck/tree/8015c448d0a97e4f9d180ae6f1192412cc308871) | `8015c448d0a97e4f9d180ae6f1192412cc308871` | MIT | Current Steamodded mod, state extraction, screenshot endpoint, stable-action waits |
| [`abhinavuppala/BalatroBotLearning`](https://github.com/abhinavuppala/BalatroBotLearning/tree/2a39dd3eabb3df6e0773d6d14b7230e8bafc1c8d) | `2a39dd3eabb3df6e0773d6d14b7230e8bafc1c8d` | No repository license found | Older UDP API and simulation-speed experiments |
| [`alesha-pro/evalatro`](https://github.com/alesha-pro/evalatro/tree/2f82617b646f88944111f91d1f8c92b05d1f6f58) | `2f82617b646f88944111f91d1f8c92b05d1f6f58` | ISC declared in `package.json`; no license file found | Cross-platform installer, isolated profile, JSONL/SQLite provenance |
| [`coder/balatrobot`](https://github.com/coder/balatrobot/tree/e7c6db8a9ad88318f6e4128eefd6e61aafc94885) | `e7c6db8a9ad88318f6e4128eefd6e61aafc94885` | MIT | Direct dependency/source of truth used by BalatroLLM and Evalatro |
| [`Steamodded/smods`](https://github.com/Steamodded/smods/tree/c7e8eb2f6daadb05ee89d72c8edfbf5094ed1eb6) | `c7e8eb2f6daadb05ee89d72c8edfbf5094ed1eb6` | GPL-3.0 | Primary source for current JSON mod metadata loading |

Repository license and game-asset rights are separate. Before publishing a
dataset or trained weights containing knowledge derived from Balatro renders,
the owner should review the distribution boundary. This note makes no claim
that an upstream code license grants rights to redistribute game assets.

## Executive conclusion

The useful seam is not any project's agent or strategy code. It is the proven
game-side integration pattern:

```text
Lovely injects into Balatro/LÖVE
  -> Steamodded loads a narrowly scoped Lua mod
  -> the mod reads card identity from G.*
  -> a localhost request stages or captures one sample
  -> the capture response returns the image identity and same-frame labels
  -> AUV records the sample as an artifact and validates it before training
```

AUV should write a small, development-only Steamodded extension mod of its own.
For the first spike it can depend on BalatroBot's server/dispatcher and register
only AUV-owned dataset endpoints; it should not fork BalatroBot, expose general
gameplay, or copy any upstream endpoint implementation. BlindDeck/BalatroBot's
separate `gamestate` and `screenshot` calls do not provide the atomic
image/label pairing required for a trustworthy training corpus.

## Repository findings

### `coder/balatrollm`

BalatroLLM contains no game-side mod. It declares `balatrobot>=1.4.1` and uses
BalatroBot's instance manager and an async JSON-RPC-over-HTTP client
([dependency](https://github.com/coder/balatrollm/blob/6269c592ac4d4534934701f42773256342457b69/pyproject.toml#L7-L14),
[client](https://github.com/coder/balatrollm/blob/6269c592ac4d4534934701f42773256342457b69/src/balatrollm/client.py#L21-L71)).
The client posts JSON-RPC requests to `http://127.0.0.1:12346/` by default.

The run loop polls named stable game states and executes typed method names such
as `start`, `gamestate`, `play`, and `cash_out`
([loop](https://github.com/coder/balatrollm/blob/6269c592ac4d4534934701f42773256342457b69/src/balatrollm/bot.py#L104-L215)).
Its screenshot is requested only after the LLM response returns and through a
separate `screenshot` call
([capture call](https://github.com/coder/balatrollm/blob/6269c592ac4d4534934701f42773256342457b69/src/balatrollm/bot.py#L250-L284)).
That is suitable for a run viewer, but the screenshot is not guaranteed to be
the same rendered frame as the previously supplied state.

Reusable ideas:

- Monotonic request IDs and structured JSON-RPC errors.
- One port per launched instance, allowing parallel data workers.
- A durable run directory that associates requests, responses, screenshots,
  and statistics.

Not reusable for this slice:

- The LLM strategy, prompt, and action loop.
- Its delayed, separate screenshot/state pairing.

### `FFFishes7/blinddeck`

BlindDeck is a BalatroBot fork. Its mod manifest declares version `1.5.2`, mod
ID `balatrobot`, and `Steamodded (>=1.~)` as its dependency
([manifest](https://github.com/FFFishes7/blinddeck/blob/8015c448d0a97e4f9d180ae6f1192412cc308871/balatrobot.json#L1-L23)).
Its installation path is the conventional stack: Lovely beside the game,
Steamodded under Balatro's `Mods/smods`, and the mod under `Mods/balatrobot`
([installation](https://github.com/FFFishes7/blinddeck/blob/8015c448d0a97e4f9d180ae6f1192412cc308871/README.md#L44-L76)).

The main Lua file loads modules with `SMODS.load_file`, registers endpoints,
initializes the server/dispatcher, and updates the non-blocking server from
`love.update`
([entrypoint](https://github.com/FFFishes7/blinddeck/blob/8015c448d0a97e4f9d180ae6f1192412cc308871/balatrobot.lua#L1-L103)).
The server is a single-client HTTP/1.1 JSON-RPC server using LuaSocket; the
dispatcher validates protocol shape, method schema, required game state, and
then endpoint execution
([server](https://github.com/FFFishes7/blinddeck/blob/8015c448d0a97e4f9d180ae6f1192412cc308871/src/lua/core/server.lua#L1-L118),
[dispatcher](https://github.com/FFFishes7/blinddeck/blob/8015c448d0a97e4f9d180ae6f1192412cc308871/src/lua/core/dispatcher.lua#L112-L224)).

The exact ground truth needed by AUV is already available inside the game:
`card.config.card.suit` and `card.config.card.value` are normalized into four
suits and thirteen ranks
([normalization](https://github.com/FFFishes7/blinddeck/blob/8015c448d0a97e4f9d180ae6f1192412cc308871/src/lua/utils/gamestate.lua#L590-L633),
[extraction](https://github.com/FFFishes7/blinddeck/blob/8015c448d0a97e4f9d180ae6f1192412cc308871/src/lua/utils/gamestate.lua#L858-L872)).
The state serializer walks `G.hand`, `G.deck`, shop, and pack areas
([areas](https://github.com/FFFishes7/blinddeck/blob/8015c448d0a97e4f9d180ae6f1192412cc308871/src/lua/utils/gamestate.lua#L1680-L1714)).
It deliberately masks identity for face-down cards, which is the correct
runtime behavior and should remain the default outside dataset staging
([masking](https://github.com/FFFishes7/blinddeck/blob/8015c448d0a97e4f9d180ae6f1192412cc308871/src/lua/utils/gamestate.lua#L976-L1042)).

The screenshot endpoint schedules `love.graphics.captureScreenshot`, encodes
the callback's `ImageData` as PNG, writes through `nativefs`, and only then
responds
([endpoint](https://github.com/FFFishes7/blinddeck/blob/8015c448d0a97e4f9d180ae6f1192412cc308871/src/lua/endpoints/screenshot.lua#L20-L75)).
However, that response contains only the output path. It does not extract
labels inside the callback, so a separate `gamestate` request is not an atomic
ground-truth pair.

BlindDeck's strongest additional idea is its event-based settlement checks.
Actions wait for meaningful conditions such as final state, complete UI,
unlocked controller, and positioned cards before returning
([play settlement](https://github.com/FFFishes7/blinddeck/blob/8015c448d0a97e4f9d180ae6f1192412cc308871/src/lua/endpoints/play.lua#L94-L179)).
The dataset mod should use similarly explicit stability predicates instead of a
fixed sleep.

### `abhinavuppala/BalatroBotLearning`

This repository embeds an older `Balatrobot-v0.3` mod using the old
`SMODS.INIT`/`NFS.read` loading style
([entrypoint](https://github.com/abhinavuppala/BalatroBotLearning/blob/2a39dd3eabb3df6e0773d6d14b7230e8bafc1c8d/balatrobot/main.lua#L1-L37)).
It binds a UDP socket to `0.0.0.0`, responds to `HELLO` polls, and accepts a
pipe-delimited action protocol
([UDP API](https://github.com/abhinavuppala/BalatroBotLearning/blob/2a39dd3eabb3df6e0773d6d14b7230e8bafc1c8d/balatrobot/src/api.lua#L1-L105),
[client](https://github.com/abhinavuppala/BalatroBotLearning/blob/2a39dd3eabb3df6e0773d6d14b7230e8bafc1c8d/balatro_connection.py#L57-L137)).
The state reader directly exports `card.config.card.suit` and `.value`, but it
does not mask face-down identities and has no screenshot endpoint
([card extraction](https://github.com/abhinavuppala/BalatroBotLearning/blob/2a39dd3eabb3df6e0773d6d14b7230e8bafc1c8d/balatrobot/src/utils.lua#L4-L47)).

Its only useful conceptual clue is the distinction between the target transform
`T` and visible transform `VT`: its optional instant-move patch assigns
`VT.x/y = T.x/y`
([speed patch](https://github.com/abhinavuppala/BalatroBotLearning/blob/2a39dd3eabb3df6e0773d6d14b7230e8bafc1c8d/balatrobot/src/api.lua#L104-L179)).
For visual annotations, AUV should record visible transforms rather than assume
the target layout is what was rendered.

Do not reuse this implementation: it has no repository license, uses an old mod
API, exposes UDP beyond loopback, omits screenshot synchronization, and contains
comments acknowledging stability problems when delays are disabled.

### `alesha-pro/evalatro`

Evalatro also delegates game access to BalatroBot. Its TypeScript client adds
timeouts, retry/backoff, structured error handling, and one-line JSONL logging
for every request/result pair
([client](https://github.com/alesha-pro/evalatro/blob/2f82617b646f88944111f91d1f8c92b05d1f6f58/src/client/balatrobot.ts#L1-L90)).
That provenance pattern is useful for dataset generation, though its exposed
client has no screenshot method.

Its installer identifies the required upstreams—Steamodded, BalatroBot, and a
Lovely release—and has explicit macOS, Windows, and Linux/Proton layouts
([dependencies](https://github.com/alesha-pro/evalatro/blob/2f82617b646f88944111f91d1f8c92b05d1f6f58/scripts/setup-local-lib.mjs#L1-L13),
[layouts](https://github.com/alesha-pro/evalatro/blob/2f82617b646f88944111f91d1f8c92b05d1f6f58/scripts/setup-local-lib.mjs#L161-L235)).
It clones the mods and installs Lovely rather than embedding game files
([install execution](https://github.com/alesha-pro/evalatro/blob/2f82617b646f88944111f91d1f8c92b05d1f6f58/scripts/setup-local-lib.mjs#L627-L636)).

The small `evalatro_unlock` mod is a good lifecycle precedent: it is gated by an
environment variable, waits until `G` and the selected profile exist, applies
once, and operates on a dedicated profile
([unlock helper](https://github.com/alesha-pro/evalatro/blob/2f82617b646f88944111f91d1f8c92b05d1f6f58/assets/evalatro_unlock/evalatro_unlock.lua#L1-L75)).
AUV's dataset mod should likewise require an explicit environment gate and a
dedicated profile so normal saves are untouched.

Evalatro launches BalatroBot with `--fast --no-shaders` for benchmark throughput
([launcher](https://github.com/alesha-pro/evalatro/blob/2f82617b646f88944111f91d1f8c92b05d1f6f58/src/game/launch.ts#L15-L46)).
Those options are unsuitable for the primary visual corpus because they change
rendering and animation behavior. They may be useful only for a separately
labeled ablation corpus.

## Current BalatroBot details that affect AUV

The current BalatroBot source supports a headed `render_on_api` mode, explicitly
separate from headless mode. Headless mode disables rendering, while
render-on-API only draws/presents when a request sets the render flag
([settings](https://github.com/coder/balatrobot/blob/e7c6db8a9ad88318f6e4128eefd6e61aafc94885/src/lua/settings.lua#L46-L75),
[headless](https://github.com/coder/balatrobot/blob/e7c6db8a9ad88318f6e4128eefd6e61aafc94885/src/lua/settings.lua#L119-L188),
[render-on-API](https://github.com/coder/balatrobot/blob/e7c6db8a9ad88318f6e4128eefd6e61aafc94885/src/lua/settings.lua#L191-L220)).
For deterministic collection, headed render-on-API plus normal shaders is the
better starting point.

On macOS, the launcher injects Lovely by setting `DYLD_INSERT_LIBRARIES` to
`liblovely.dylib` and launches Balatro's bundled LÖVE executable
([launcher](https://github.com/coder/balatrobot/blob/e7c6db8a9ad88318f6e4128eefd6e61aafc94885/src/balatrobot/platforms/macos.py#L10-L47)).
That confirms an automated local launch is feasible without modifying the game
bundle beyond installing Lovely and the Steamodded mods.

## Recommended AUV-owned dataset seam

### Scope classification

This would be an approved feature only if the owner accepts a development-only
Balatro data-generation surface. It should not become a gameplay dependency or
a public AUV support claim. The output is training evidence, not proof that the
production visual reader works.

### Mod boundary

Create a small Steamodded mod with its own ID and prefix, for example provisional
`auv_dataset`. For the first integration spike, make BalatroBot `>=1.5.2` an
explicit dependency and register AUV-owned endpoint tables through
`BB_DISPATCHER.register`; the dispatcher supports post-initialization endpoint
registration by name
([registration](https://github.com/coder/balatrobot/blob/e7c6db8a9ad88318f6e4128eefd6e61aafc94885/src/lua/core/dispatcher.lua#L44-L72)).
This reuses the MIT-licensed installed component without copying its server. A
standalone AUV server should be a later, separately approved slice only if the
dataset tool must run without BalatroBot.

The mod should:

- Depend on Steamodded and BalatroBot, but not on an agent/player project.
- Load only when `AUV_BALATRO_DATASET=1`.
- Bind only to `127.0.0.1` on a configurable port.
- Refuse requests outside a dedicated profile and dataset mode.
- Expose no arbitrary Lua evaluation, save mutation, money setting, or gameplay
  actions.
- Write only beneath a configured output root, rejecting traversal and absolute
  paths outside that root.
- Include `schema_version`, mod version, Balatro version, Steamodded version,
  render settings, and run/sample IDs in every response.

The current Steamodded loader requires JSON metadata fields `id`, `author`,
`name`, `description`, `prefix`, and `main_file`. It validates the referenced
main file before loading it
([loader schema](https://github.com/Steamodded/smods/blob/c7e8eb2f6daadb05ee89d72c8edfbf5094ed1eb6/src/preflight/loader.lua#L135-L153),
[main-file validation](https://github.com/Steamodded/smods/blob/c7e8eb2f6daadb05ee89d72c8edfbf5094ed1eb6/src/preflight/loader.lua#L301-L354)).
A minimal AUV manifest should therefore have this shape:

```json
{
  "id": "auv_dataset",
  "name": "AUV Dataset Capture",
  "author": ["moeru-ai"],
  "description": "Development-only synchronized Balatro dataset capture.",
  "prefix": "AUVD",
  "main_file": "main.lua",
  "version": "0.1.0",
  "priority": 0,
  "dependencies": ["Steamodded (>=1.~)", "balatrobot (>=1.5.2)"]
}
```

Steamodded executes the referenced Lua file directly after config and
dependency checks
([load path](https://github.com/Steamodded/smods/blob/c7e8eb2f6daadb05ee89d72c8edfbf5094ed1eb6/src/preflight/loader.lua#L767-L780)).
This is the current JSON-manifest path; the old `--- STEAMODDED HEADER` form in
BalatroBotLearning should not be used.

### Minimal API shape

The exact names remain provisional until implementation review.

```json
{"jsonrpc":"2.0","method":"dataset.health","params":{},"id":1}
```

Returns versions, active profile, render settings, and whether capture is
available.

```json
{
  "jsonrpc": "2.0",
  "method": "dataset.stage",
  "params": {
    "sample_id": "base-000001",
    "cards": [
      {"rank": "A", "suit": "S"},
      {"rank": "2", "suit": "H"}
    ],
    "highlighted": [],
    "layout_seed": 17
  },
  "id": 2
}
```

Stages a bounded hand from typed rank/suit values using game-native card
construction, then waits for explicit stability. It returns a `stage_token`,
not an image.

```json
{
  "jsonrpc": "2.0",
  "method": "dataset.capture",
  "params": {
    "stage_token": "...",
    "relative_path": "frames/base-000001.png"
  },
  "id": 3
}
```

The capture response should be produced from the
`love.graphics.captureScreenshot` callback and contain:

```json
{
  "schema_version": 1,
  "sample_id": "base-000001",
  "stage_token": "...",
  "frame_seq": 42,
  "image": {"path": "frames/base-000001.png", "width": 1920, "height": 1080},
  "state": "SELECTING_HAND",
  "cards": [
    {
      "area": "hand",
      "index": 0,
      "instance_id": 123,
      "rank": "A",
      "suit": "S",
      "hidden": false,
      "highlighted": false,
      "target_transform": {"x": 0, "y": 0, "w": 0, "h": 0},
      "visible_transform": {"x": 0, "y": 0, "w": 0, "h": 0}
    }
  ]
}
```

The mod should extract labels and visible transforms inside that callback, not
in an earlier `gamestate` request. Pixel-coordinate conversion is intentionally
deferred until it is validated against an overlay or known single-card fixture;
raw `T`/`VT` values should not be called pixel bounds prematurely.

`dataset.clear` may be added only if staging cannot replace the previous bounded
hand atomically. Do not add a general-purpose `exec`, `set`, or `add` endpoint.

### Stability and pairing rules

A sample is accepted only when all applicable conditions hold:

- Dataset mode and dedicated profile are active.
- The staged token still matches the current hand.
- `G.STATE_COMPLETE == true`.
- `G.CONTROLLER.locked` is false.
- The hand exists, is not removed, and every card has `T` and `VT` transforms.
- The requested card count equals the returned label count.
- The screenshot write succeeds and its dimensions are recorded.
- The external AUV detector returns the same number of ordered card slots; if
  order-to-slot association is used, any ambiguity rejects the sample.

The runner should use a bounded retry/timeout and persist rejection reasons.
Fixed sleeps are not evidence that the frame settled.

### Balanced generation schedule

The external AUV runner, not Lua, should own the schedule and manifest. A useful
first corpus is:

1. Balance all 52 `(rank, suit)` combinations exactly.
2. Randomize order and neighboring cards so class and screen position are not
   correlated.
3. Cross controlled nuisance factors: window/render scale, 5–8 card overlap,
   highlighted/unhighlighted, debuff, enhancement, edition, seal, and normal
   shader effects.
4. Keep clean base cards as a separately measurable stratum.
5. Split train/validation/test by capture run and nuisance configuration, not by
   adjacent crops from the same frame.
6. Store a manifest row per crop with image hash, source-frame hash, label,
   stage request, render settings, mod/source versions, detector association,
   and rejection history.

This makes every captured example fully labeled without pretending every
possible modifier combination needs equal weight. Class balance and nuisance
coverage should be explicit, reviewable distributions.

## Recommended first slice

1. Install/verify Lovely, Steamodded, and current BalatroBot using the current
   machine's platform path, without installing or modifying any reviewed agent
   project.
2. Build the AUV-owned mod with only `dataset.health`, `dataset.stage`, and
   atomic `dataset.capture`.
3. Generate a 52-card clean corpus in small hands, normal shaders, and one fixed
   resolution.
4. Run AUV's existing card detector/corner cropper, reject count mismatches,
   and write a manifest.
5. Manually inspect a stratified sample and run schema/hash/inventory checks.
6. Only after this gate, add controlled visual nuisances and train/evaluate the
   existing Balatro corner classifier.

The highest-risk issue is not card identity extraction; that is straightforward.
It is proving that each crop, screenshot, and label refer to the same rendered
frame and preserving that lineage in AUV's run artifacts.
