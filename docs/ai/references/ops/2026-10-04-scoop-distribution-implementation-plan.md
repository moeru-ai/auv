# Scoop Distribution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the existing Windows x86-64 AUV release installable from this repository through Scoop.

**Architecture:** Check in one portable Scoop manifest and let ScoopInstaller's official Excavator action update it when a stable GitHub release is published. Keep Scoop policy in the manifest and official action; add no AUV-owned renderer, wrapper, or update test harness.

**Tech Stack:** Scoop manifest JSON, GitHub Actions, ScoopInstaller/GithubActions Excavator, Markdown.

**Spec:** `docs/ai/references/ops/2026-10-04-scoop-distribution-design.md`

## Global Constraints

- Support only `auv-x86_64-pc-windows-msvc.zip`; do not claim Windows ARM64.
- Pin `ScoopInstaller/GithubActions` to `5bceeda181721d9d10a18fdcc3ddca6f33467ae6` (`v3.0.0`), the revision used by the current official BucketTemplate.
- Trigger automatic updates only for stable releases with `release.types: [released]`; retain `workflow_dispatch` for recovery.
- Checkout `main` explicitly because a release workflow otherwise runs from a tag ref.
- Use the workflow `GITHUB_TOKEN`; do not add a PAT or another repository.
- Do not add a renderer, wrapper script, pre/post-install hook, shortcut, persistence rule, or custom autoupdate implementation.
- Preserve the README statement that Cargo/source-built macOS binaries do not embed the signed `AUV Helper.app`.

## Review Focus

- A stable release URL must include the `v` prefix while Scoop's `$version` remains plain semver; Task 1's JSON assertions and local install cover the resulting URL.
- The companion `.sha256` contains a bare lowercase digest; Task 1's local install must prove Scoop accepts the `autoupdate.hash.url` shape.
- Release checkout must not be detached at the tag; Task 1 statically asserts `ref: main` in the workflow.
- A prerelease must not update the bucket; Task 1 statically asserts that the workflow listens only for `released`.
- The README must not imply Scoop or Windows ARM64 support beyond the one published x86-64 archive; Task 2 reviews the rendered installation section.

---

### Task 1: Scoop manifest and official updater

**Files:**
- Create: `bucket/auv.json`
- Create: `.github/workflows/update-scoop.yml`

**Interfaces:**
- Consumes: stable GitHub releases containing `auv-x86_64-pc-windows-msvc.zip` and its `.sha256` companion asset.
- Produces: the Scoop package name `auv/auv`, with `auv.exe` available through Scoop's shim, and an Excavator-maintained manifest on `main`.

- [ ] **Step 1: Add the initial portable manifest**

Create `bucket/auv.json` for version `0.0.25` with these exact values:

- `description`: `Invoke and inspect core computer-use capabilities`
- `homepage`: `https://github.com/moeru-ai/auv`
- `license`: `Apache-2.0`
- `url`: `https://github.com/moeru-ai/auv/releases/download/v0.0.25/auv-x86_64-pc-windows-msvc.zip`
- `hash`: `4da4a39db5e3e00de3b5df6403baa4b8a372bfe69dd66deaf550482c90e61a7e`
- `bin`: `auv.exe`
- `checkver.github`: `https://github.com/moeru-ai/auv`
- `autoupdate.url`: `https://github.com/moeru-ai/auv/releases/download/v$version/auv-x86_64-pc-windows-msvc.zip`
- `autoupdate.hash.url`: `$url.sha256`

- [ ] **Step 2: Verify the initial manifest fields and release checksum**

Run:

```sh
jq -e '
  .version == "0.0.25" and
  .url == "https://github.com/moeru-ai/auv/releases/download/v0.0.25/auv-x86_64-pc-windows-msvc.zip" and
  .hash == "4da4a39db5e3e00de3b5df6403baa4b8a372bfe69dd66deaf550482c90e61a7e" and
  .bin == "auv.exe" and
  .autoupdate.hash.url == "$url.sha256"
' bucket/auv.json
test "$(curl -fsSL https://github.com/moeru-ai/auv/releases/download/v0.0.25/auv-x86_64-pc-windows-msvc.zip.sha256 | tr -d '\r\n')" = "$(jq -r .hash bucket/auv.json)"
```

Expected: both commands exit `0` and print `true` from `jq`.

- [ ] **Step 3: Add the Excavator workflow**

Create `.github/workflows/update-scoop.yml` with one Windows job. Configure `release.types: [released]`, `workflow_dispatch`, `permissions.contents: write`, `actions/checkout@v6` with `ref: main`, and the pinned official Excavator action. Set `GITHUB_TOKEN` to `${{ secrets.GITHUB_TOKEN }}`, `SKIP_UPDATED` to `1`, and `THROW_ERROR` to `1`.

- [ ] **Step 4: Verify the workflow's fixed policy values**

Run:

```sh
rg -n 'types:.*released|workflow_dispatch|contents: write|ref: main|ScoopInstaller/GithubActions@5bceeda181721d9d10a18fdcc3ddca6f33467ae6|THROW_ERROR: 1' .github/workflows/update-scoop.yml
git diff --check -- bucket/auv.json .github/workflows/update-scoop.yml
```

Expected: every fixed policy value is present and `git diff --check` exits `0`.

- [ ] **Step 5: Exercise the manifest through Scoop on Windows**

From a Windows checkout with Scoop installed, run:

```powershell
scoop install bucket\auv.json
auv --version
scoop uninstall auv
```

Expected: installation succeeds, `auv --version` prints `auv 0.0.25`, and uninstall succeeds. Record the command result in the PR verification section; do not add a permanent wrapper solely for this check.

- [ ] **Step 6: Commit the distribution files**

```sh
git add bucket/auv.json .github/workflows/update-scoop.yml
git commit -m "feat(auv-cli): add Scoop distribution"
```

### Task 2: Getting Started documentation

**Files:**
- Modify: `README.md`

**Interfaces:**
- Consumes: the `auv/auv` Scoop package produced by Task 1.
- Produces: a Windows Getting Started path that recommends Scoop while retaining direct-release and Cargo alternatives.

- [ ] **Step 1: Add Scoop as the primary Windows install path**

Under `Getting Started > Install > Windows`, place these commands before the direct ZIP instructions:

```powershell
scoop bucket add auv https://github.com/moeru-ai/auv
scoop install auv/auv
auv --version
```

State that the current Scoop manifest supports Windows x86-64. Keep the direct GitHub Release instructions as the package-manager-free alternative and keep `Install with Cargo > Windows` unchanged.

- [ ] **Step 2: Regenerate documentation tables of contents**

Run:

```sh
pnpm docs:update
```

Expected: doctoc reports `Everything is OK` for each managed README.

- [ ] **Step 3: Verify the documented commands and support boundary**

Run:

```sh
rg -n 'scoop bucket add auv|scoop install auv/auv|Windows x86-64|auv setup macos-helper install|do not embed the signed helper app' README.md
git diff --check -- README.md
```

Expected: every command and boundary is present and `git diff --check` exits `0`.

- [ ] **Step 4: Commit the documentation**

```sh
git add README.md
git commit -m "docs(auv-cli): document installation and macOS setup"
```

### Task 3: Final verification

**Files:**
- Verify only; no new files.

**Interfaces:**
- Consumes: Tasks 1 and 2.
- Produces: PR-ready evidence for the complete Scoop slice.

- [ ] **Step 1: Run repository-level documentation checks**

```sh
pnpm docs:update
git diff --check
git status --short
```

Expected: doctoc reports `Everything is OK`, `git diff --check` exits `0`, and status contains no unintended files.

- [ ] **Step 2: Review the branch diff against the design**

```sh
git diff HEAD~2 -- README.md bucket/auv.json .github/workflows/update-scoop.yml
git log -2 --oneline
```

Expected: the diff contains only the manifest, official-action workflow, and Getting Started documentation; the two commits match Tasks 1 and 2.
