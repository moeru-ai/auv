# Scoop Distribution Design

Status: approved design for the first installation-distribution PR on
2026-10-04.

## Goal

Make the existing Windows x86-64 release archive installable from the AUV
repository as a Scoop bucket:

```powershell
scoop bucket add auv https://github.com/moeru-ai/auv
scoop install auv/auv
```

The manifest must follow stable AUV releases without adding a project-owned
manifest renderer or duplicating Scoop's update behavior.

## Scope

The PR adds three user-facing pieces:

1. `bucket/auv.json`, a portable manifest for
   `auv-x86_64-pc-windows-msvc.zip`;
2. one small release-triggered workflow that runs ScoopInstaller's official
   Excavator action; and
3. the Scoop commands in the Windows installation section of `README.md`.

The manifest exposes `auv.exe` through Scoop's `bin` property. It includes the
package description, homepage, Apache-2.0 license, stable version, release URL,
and SHA-256 hash. Its `checkver` reads AUV's GitHub releases, while
`autoupdate` derives the versioned Windows archive URL and reads the published
`.sha256` companion asset.

## Update workflow

The workflow responds to a stable GitHub release (`release: released`) and to
manual dispatch. It grants `contents: write`, checks out `main` explicitly,
and invokes the official
[`ScoopInstaller/GithubActions`](https://github.com/ScoopInstaller/GithubActions)
Excavator action on a Windows runner. The action is pinned to the immutable
revision used by ScoopInstaller's current
[`BucketTemplate`](https://github.com/ScoopInstaller/BucketTemplate/blob/master/.github/workflows/excavator.yml),
not to a moving branch. It uses the workflow's `GITHUB_TOKEN`, enables
`SKIP_UPDATED`, and enables `THROW_ERROR` so update failures fail the run.

Excavator owns version detection, manifest rewriting, hash retrieval, and the
resulting commit. AUV does not add a renderer, a wrapper script, or a parallel
manifest-update implementation.

## Validation and failure behavior

The checked-in manifest uses Scoop's standard schema and only portable archive
fields; it needs no install or uninstall hooks. The existing release matrix
already builds the Windows archive, publishes its checksum, and executes the
resulting `auv.exe --help` before publication. Excavator fails its workflow if
the release, autoupdate definition, URL, or hash cannot be resolved, leaving
the previous manifest in place.

The initial PR should record one Windows installation check using the local
manifest and verify `auv --version`. This is implementation evidence, not a
new permanent project-owned test harness.

## Deliberate exclusions

- Windows ARM64 is not claimed because AUV publishes no Windows ARM64 archive.
- The PR does not add shortcuts, persistence, pre/post-install hooks, or a
  standalone installer.
- The PR does not add a custom renderer or renderer tests.
- Scoop registry or directory submission is separate from using this
  repository as a bucket.
- proto, Nix, and Unix bootstrap installation remain separate approved slices.
