# Playwright-inspired @auv-js/sdk interface research

Date: 2026-08-13

Status: design research, not an accepted API contract

This note studies the parts of Playwright's public API that are useful for a
typed `@auv-js/sdk` interface. It uses Playwright's official documentation and
source only. The recommendation is to borrow the locator model, not to copy
browser-specific DOM semantics.

## The important Playwright idea

Playwright describes locators as the center of its auto-waiting and retry
behavior. A locator is a recipe for finding an element "at any moment," rather
than a captured element returned by an earlier query. The client-side
implementation keeps a frame plus a selector, and composes new selectors when
methods such as `locator()`, `getByRole()`, and `filter()` are called
([locator guide](https://playwright.dev/docs/locators),
[client source](https://github.com/microsoft/playwright/blob/main/packages/playwright-core/src/client/locator.ts)).

This enables an API shaped like:

```ts
const submit = page
  .getByRole('dialog')
  .getByRole('button', { name: 'Submit' })

await submit.click()
```

Constructing `submit` does not perform the click or permanently bind a stale
element. The target is resolved when the action runs.

That separation is more useful than a direct `clickText(text, target,
options)` API because one target description can support many operations:
`click`, `hover`, `boundingBox`, `screenshot`, `waitFor`, assertions, and
inspection. It also gives filtering, strictness, retries, and diagnostics one
shared owner.

## Locator construction and refinement

Playwright recommends user-facing locators such as `getByRole`, `getByText`,
`getByLabel`, `getByPlaceholder`, `getByAltText`, `getByTitle`, and
`getByTestId`. `getByRole` uses ARIA role, ARIA state, and accessible name
([locator guide](https://playwright.dev/docs/locators),
[`getByRole` API](https://playwright.dev/docs/api/class-locator#locator-get-by-role)).

Locators can be refined without performing a query:

- calling a locator method from another locator searches within the outer
  locator's subtree;
- `filter({ has, hasNot, hasText, hasNotText, visible })` narrows matches, and
  nested locators are relative to the outer match;
- `and()` intersects two locators and `or()` unions them;
- `first()`, `last()`, and `nth()` make positional selection explicit.

The relative-scope requirements and composition behavior are documented in the
official [`Locator` API](https://playwright.dev/docs/api/class-locator#locator-filter).

For AUV, the valuable principle is that selection terms are immutable,
composable values. The DOM-specific meaning of "subtree" must not be assumed
for pixels. Relative chaining is valid only when the selected evidence source
has a real containment model, such as an accessibility tree or a recognized
region.

## Strictness

Playwright locators are strict for operations that require one target. A click
throws when more than one element matches, while multi-element operations such
as `count()` intentionally accept several matches. Playwright permits
`first()`, `last()`, and `nth()`, but its guide discourages using them as the
default fix because a changed page may make the same position refer to another
element
([strictness documentation](https://playwright.dev/docs/locators#strictness)).

An AUV locator action should likewise have these distinct outcomes:

- no candidate appeared before the timeout;
- exactly one candidate passed selection and actionability;
- multiple candidates remained, so the action failed with evidence describing
  the ambiguity;
- the caller explicitly selected a candidate with `nth()` or an equivalent
  refinement.

Silently choosing the first OCR match would discard Playwright's most important
safety property.

## Auto-waiting and actionability

Before `locator.click()`, Playwright waits for the locator to resolve to exactly
one element and checks that it is visible, stable, receives events, and is
enabled. Failure to satisfy the checks before the timeout produces a
`TimeoutError`. `force` disables some non-essential checks, while `trial`
performs checks without the action
([actionability documentation](https://playwright.dev/docs/actionability),
[`Locator.click` API](https://playwright.dev/docs/api/class-locator#locator-click)).

The equivalent AUV policy cannot simply reuse those words as claims. AUV may be
able to establish, depending on its evidence source:

- one match exists in the selected window or display;
- the match remains spatially stable across observations;
- the owning window still exists and satisfies foreground/input policy;
- a semantic accessibility node reports enabled/actionable state;
- the intended click point remains inside the selected target and capture.

Pixel recognition alone generally cannot prove that a target is enabled,
unobscured, or will receive the input. Those checks must report their actual
evidence level rather than presenting a Playwright-like name as proof.

`trial: true` is still a useful inspiration for AUV: it could resolve the
locator and run supported readiness checks without delivering input. `force`
should bypass only named checks and should remain visible in the action result
and trace.

## Scope: Page and Frame versus Window and Screen

Playwright creates locators from `Page`, `Frame`, `FrameLocator`, and existing
locators. A `FrameLocator` stores enough information to enter an iframe, and is
itself strict when the frame selector matches more than one frame
([`FrameLocator` API](https://playwright.dev/docs/api/class-framelocator)).
Playwright also discourages many older `frame.click(selector)`-style APIs in
favor of locator-based operations
([`Frame` API](https://playwright.dev/docs/api/class-frame)).

The corresponding AUV design should bind the search scope before the selector:

```ts
const runner = auv.runner({ runnerClass, deviceId, runId })
const window = runner.window({ bundleId: 'com.example.App', title: /Setup/ })

const continueButton = window.getByRole('button', { name: 'Continue' })
await continueButton.click({ signal, timeout: 5_000 })
```

For a visual-only surface:

```ts
const primaryDisplay = runner.display({ primary: true })
await primaryDisplay.getByText('Continue').click({ signal })
```

This avoids an options bag containing mutually exclusive `window?` and
`screen?` fields. The scope object owns routing, coordinates, capture source,
and default policy; the locator owns target selection; the action owns click
options.

`getByRole()` should mean accessibility/semantic selection. `getByText()` needs
an explicit AUV contract: window accessibility text, OCR text, or an ordered
fallback between them are observably different. A source-specific alternative
such as `getByRecognizedText()` may be clearer until that policy is accepted.

## Action options, cancellation, and results

Playwright's click accepts action-specific options including mouse button,
click count, delay, modifiers, position, `force`, `trial`, timeout, and
`AbortSignal`; it returns `Promise<void>`. Its current API states that an abort
signal cancels the operation and does not disable the independent timeout
([`Locator.click` API](https://playwright.dev/docs/api/class-locator#locator-click)).
Default timeouts can be placed on `Page` or `BrowserContext`
([`Page.setDefaultTimeout`](https://playwright.dev/docs/api/class-page#page-set-default-timeout)).

AUV should borrow the split between context defaults and per-action overrides:

```ts
await locator.click({
  button: 'left',
  clickCount: 1,
  signal,
  timeout: 5_000,
  trial: false,
})
```

It should not necessarily copy `Promise<void>`. AUV's current contract values
typed delivery facts, disturbance metadata, trace events, and artifacts, so a
`Promise<ClickResult>` (or the existing typed input result) is more consistent.
The locator recipe and the resolved candidate evidence can be attached to that
result without weakening the Playwright-like call shape.

Cancellation also needs a narrower claim than browser DOM automation. Aborting
while resolving, observing, or waiting can stop those stages. Once native input
has been delivered to an operating system or remote Runner, aborting the client
wait cannot guarantee that the click did not occur. The result/error contract
should expose whether delivery had started.

## Recommended AUV mapping

| Playwright concept | AUV analogue | Owner |
| --- | --- | --- |
| `BrowserContext` / connection defaults | authenticated Runner binding and default timeout/input policy | @auv-js/sdk client context |
| `Page` | resolved or lazily selected application window | window capability |
| `FrameLocator` | a narrower semantic or visual region with a real containment relation | locator/scope implementation |
| `Locator` | immutable, lazy target-selection recipe | shared typed operation contract plus @auv-js/sdk facade |
| `getByRole` | accessibility role/name/state query | accessibility capability |
| `getByText` | provisional text-selection policy; do not silently conflate AX and OCR | owning recognition/selection contract |
| `filter`, chaining | relative refinement within a proven scope | locator contract |
| strict action | require exactly one actionable candidate | shared execution policy |
| actionability | evidence-backed readiness checks supported by the chosen source | shared operation plus drivers |
| `click(options)` | target resolution followed by typed input delivery | shared operation; driver owns delivery |
| trace/debug output | run trace, artifacts, candidates, attempts, and `InputActionResult` | `auv-tracing` and owning operation |

The recommended public direction is therefore:

```ts
await runner
  .window({ bundleId: 'com.example.App' })
  .getByRole('button', { name: 'Continue' })
  .click({ signal, timeout: 5_000 })
```

not a growing family of `clickText`, `clickRole`, `hoverText`, and
`screenshotText` helpers. A convenience `clickText()` could exist later as a
thin spelling of `getByText(text).click(options)`, but it should not own a
separate matching, waiting, routing, or input policy.

## Deliberate gaps

- **TODO(locator-contract):** the stable serialized selector/locator shape is
  deferred because this research does not approve a new public operation; the
  decision reopens when an owner approves the typed locator slice.
- **TODO(text-source-policy):** `getByText` source ordering between
  accessibility and OCR is deferred until the owning recognition contract and
  evidence requirements are selected.
- **TODO(actionability-policy):** exact readiness checks and retry observation
  rules are deferred until each driver capability can name the evidence it can
  actually provide.

