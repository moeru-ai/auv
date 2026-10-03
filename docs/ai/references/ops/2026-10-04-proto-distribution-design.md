# AUV proto Distribution Design

Date: 2026-10-04

Status: Approved for the first-party integration slice.

## Goal

Install AUV release archives through [proto](https://moonrepo.dev/docs/proto)
on every platform that AUV currently publishes.

This slice keeps the plugin in the AUV repository. It does not submit AUV to
the proto community registry.

## Scope

Add one non-WASM plugin at `toolchain/proto/auv.toml`. Add an
`Install with proto` section to `README.md`.

Do not add a generator, update script, or release workflow. The plugin uses
templates for the version, architecture, archive name, and checksum name.

The plugin supports these release targets:

| Platform | Architectures | Release format |
| --- | --- | --- |
| macOS | `aarch64`, `x86_64` | `.tar.gz` |
| GNU Linux | `aarch64`, `x86_64` | `.tar.gz` |
| Windows | `x86_64` | `.zip` |

The plugin does not claim Linux musl, Windows ARM64, 32-bit systems, or BSD.
AUV does not publish archives for these targets.

## Plugin contract

The plugin follows proto's
[non-WASM plugin format](https://moonrepo.dev/docs/proto/non-wasm-plugin).
It defines one platform entry for Linux, macOS, and Windows.

Each platform entry declares its supported architectures, archive name,
checksum name, and executable path. The install section downloads files from
the matching GitHub release.

The Linux archive template uses `{libc}`. A GNU host resolves the current
`*-gnu.tar.gz` archive. A musl host resolves an unavailable asset and stops.

The resolve section reads semantic versions from AUV Git tags. An anchored
version pattern accepts `v<major>.<minor>.<patch>` tags and prerelease suffixes.

AUV release assets contain the executable at the archive root. The plugin does
not need an archive prefix or an install hook.

## User flow

Before registry inclusion, users must add the plugin locator:

```sh
proto plugin add auv \
  "https://raw.githubusercontent.com/moeru-ai/auv/main/toolchain/proto/auv.toml" \
  --to global
proto install auv --config-mode global --pin global
auv --version
```

The explicit configuration mode lets the install command read the global
plugin locator. `proto setup` remains part of proto's own installation flow.

Projects can put the locator and an exact AUV version in `.prototools`. This
slice does not add a `.prototools` file to the AUV repository.

## Release and helper boundaries

The plugin downloads the same five archives that the AUV release workflow
publishes. It also downloads each matching `.sha256` file.

The macOS archive includes the signed and notarized `AUV Helper.app`. The proto
path therefore preserves the official macOS release boundary.

Git tags can appear before all release assets finish uploading. During this
short interval, an install of `latest` can fail. The plugin does not add retry
or fallback policy for this release-state boundary.

## Validation

Make sure that the TOML matches all five current release asset names. Make sure
that unsupported architectures are absent from the plugin.

Install AUV through proto on macOS, GNU Linux, and Windows. Run
`proto run auv -- --version` on each system.

Use a released version for these checks. This avoids the tag and asset upload
interval.

## Deliberate exclusions

- Do not submit a copy to `moonrepo/community-plugins` in this slice.
- Do not add a WASM plugin.
- Do not add an AUV-owned installer or plugin generator.
- Do not add a new release workflow or a manifest update workflow.
- Do not add compatibility branches for release assets that do not exist.

Registry inclusion can be a separate owner-approved change. It is not required
for installation through the repository-hosted plugin.

## Evidence

- [proto plugins](https://moonrepo.dev/docs/proto/plugins)
- [proto non-WASM plugin format](https://moonrepo.dev/docs/proto/non-wasm-plugin)
- [`proto plugin add`](https://moonrepo.dev/docs/proto/commands/plugin/add)
- [`proto install`](https://moonrepo.dev/docs/proto/commands/install)
- [alint proto manifest](https://github.com/moeru-ai/alint/blob/main/toolchain/proto/alint.toml)
- [AUV release workflow](../../../.github/workflows/release.yml)

