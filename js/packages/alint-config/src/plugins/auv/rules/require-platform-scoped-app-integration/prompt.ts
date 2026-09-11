export const platformScopedAppIntegrationPrompt = `
AUV app integrations must keep platform-owned behavior behind a visible platform boundary.

Apply these conventions to application crates that integrate AUV with a desktop app:

- Production code that calls an AUV driver, invokes operating-system application tooling, or directly automates a computer or application belongs under src/platforms/.
- Each cohesive capability and platform combination belongs in one file named <capability>_<platform>.rs, such as search_macos.rs or playback_windows.rs. The capability name should describe the app behavior, not the driver method used to implement one step.
- Keep one platform workflow cohesive. Driver setup, platform UI interpretation, input delivery, platform fallback policy, and platform verification for the same capability should not be scattered across thin wrappers or one-step helper files.
- Platform-specific view parser acquisition and adapters follow the same placement and file naming convention. Pure parser contracts, platform-neutral reconstruction, and reusable typed IR may remain outside src/platforms/.
- CLI parsing and presentation stay outside src/platforms/. Platform-neutral typed requests, results, and artifact mappings may also stay outside when they do not depend on platform APIs.
- Shared AUV driver crates and platform-neutral runtime/framework crates are not app integrations and do not need this app-crate layout.

Report only concrete violations demonstrated by files in the inspected crate. Use one of these categories:

- automation-placement: platform-owned automation or operating-system app tooling lives outside src/platforms/.
- platform-file: a platform implementation lacks a clear <capability>_<platform>.rs owner.
- scattered-wrapper: one platform capability is fragmented into thin wrappers or driver-step files instead of one cohesive workflow owner.
- view-parser-platform: platform-specific view parser acquisition, adaptation, or UI interpretation is mixed into a platform-neutral parser module.

Do not report naming taste, file size by itself, test code, generated code, platform-neutral domain logic, or a cohesive private helper that supports its owning platform workflow. When evidence is incomplete, submit no finding.
`.trim()
