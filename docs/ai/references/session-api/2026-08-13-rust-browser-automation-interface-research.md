# Rust browser automation interface research

Date: 2026-08-13

Status: design research, not an accepted API contract

This note compares current Rust browser automation APIs as references for a
Playwright-inspired AUV Rust interface. It uses project-owned documentation,
published crate source, and the official Playwright language list.

## Short answer

Microsoft does not publish a Rust Playwright binding. Its supported-language
page lists JavaScript/TypeScript, Python, Java, and .NET, but not Rust
([Playwright supported languages](https://playwright.dev/docs/languages)). Rust
has community bindings and browser-automation libraries instead.

The closest current API reference is `playwright-rs` (`playwright_rs` in Rust),
not the older `playwright` crate. `playwright-rs` 0.15.1 was published on
2026-08-02 and describes itself as pre-1.0 with a stabilizing API
([release history and status](https://docs.rs/crate/playwright-rs/latest)). It
uses the familiar hierarchy:

```text
Playwright -> BrowserType -> Browser -> BrowserContext -> Page -> Locator
```

It talks JSON-RPC over stdio to the Node Playwright server, which then speaks
the browsers' native protocols
([architecture](https://docs.rs/crate/playwright-rs/latest#how-it-works)).

The crate named `playwright` is an earlier community port. Its newest release is
0.0.20 from 2022-08-20
([release history](https://docs.rs/crate/playwright/latest)); its API is mainly
`Page`/`Frame` plus returned `ElementHandle` values and option builders rather
than the current Locator-first design
([public API](https://docs.rs/playwright/latest/playwright/api/index.html),
[`ElementHandle`](https://docs.rs/playwright/latest/playwright/api/element_handle/struct.ElementHandle.html)).
It remains useful as historical evidence for Rust builders, but it is not the
best current model.

## API comparison

| Crate | Current evidence | Protocol and public shape | Selection, waiting, action |
| --- | --- | --- | --- |
| `playwright-rs` | 0.15.1, 2026-08-02; pre-1.0, stabilizing ([docs.rs](https://docs.rs/crate/playwright-rs/latest)) | Node Playwright server over JSON-RPC; `BrowserContext -> Page -> Locator` ([architecture](https://docs.rs/crate/playwright-rs/latest#how-it-works)) | `Locator` is a lightweight cloneable recipe holding frame, selector, and page; `first`, `last`, `nth`, `get_by_text`, `get_by_role`, subtree `locator`, and `filter` return new locators ([source](https://docs.rs/playwright-rs/latest/src/playwright_rs/protocol/locator.rs.html#663-903)). Actions delegate with `strict=true`; `click(options)` returns `Result<()>`, inherits the page timeout, and relies on Playwright auto-wait/actionability ([source](https://docs.rs/playwright-rs/latest/src/playwright_rs/protocol/locator.rs.html#1141-1178)). |
| `thirtyfour` | 0.37.5, 2026-08-12 ([docs.rs](https://docs.rs/crate/thirtyfour/latest)) | W3C WebDriver, with optional typed CDP and WebDriver BiDi; `WebDriver -> ElementQuery -> WebElement` ([crate docs](https://docs.rs/crate/thirtyfour/latest)) | Its query builder can scope from driver or element, add filters/alternatives, poll with per-query timeout/interval, and resolve as `single`, `first`, or `all`; current examples use `single` for uniqueness ([query guide](https://docs.rs/thirtyfour/latest/thirtyfour/extensions/query/index.html), [crate example](https://docs.rs/thirtyfour/latest/thirtyfour/)). Readiness is a separate `wait_until().displayed()/enabled()/clickable()` API, and `WebElement::click()` returns `Result<()>` ([waiter source](https://docs.rs/thirtyfour/latest/src/thirtyfour/extensions/query/element_waiter.rs.html), [`WebElement`](https://docs.rs/thirtyfour/latest/thirtyfour/struct.WebElement.html)). `click_when_ready` explicitly does not re-resolve stale elements, retry a failed click, or prove complete interactability ([method contract](https://docs.rs/thirtyfour/latest/thirtyfour/struct.WebElement.html#method.click_when_ready)). |
| `fantoccini` | 0.22.1, 2026-02-28 ([docs.rs](https://docs.rs/crate/fantoccini/latest)) | W3C WebDriver; one cloneable `Client` per browser session, `Client/Element::find(Locator) -> Element` ([`Client`](https://docs.rs/fantoccini/latest/fantoccini/client/struct.Client.html), [`Element`](https://docs.rs/fantoccini/latest/fantoccini/elements/struct.Element.html)) | `Locator` is only a borrowed selector enum (`Css`, `Id`, `LinkText`, `XPath`), not a lazy retained locator recipe ([`Locator`](https://docs.rs/fantoccini/latest/fantoccini/enum.Locator.html)). Waiting is explicit: `client.wait()` polls every 250 ms for up to 30 s by default and can override both; click returns `Result<()>` ([wait module](https://docs.rs/fantoccini/latest/fantoccini/wait/index.html)). |
| `chromiumoxide` | 0.9.1, 2026-02-25 ([docs.rs](https://docs.rs/crate/chromiumoxide/latest)) | Async Chromium-only CDP; `Browser -> Page -> Element`, with a separately polled handler task ([usage](https://docs.rs/crate/chromiumoxide/latest#usage)) | `find_element` resolves the first CSS match immediately into a node/object handle; element methods return `&Self`, enabling action-result chaining such as `click().await?.type_str(...).await?` ([`Element`](https://docs.rs/chromiumoxide/latest/chromiumoxide/element/struct.Element.html)). There is no Locator strictness/actionability layer; navigation waits are explicit after click ([`Page`](https://docs.rs/chromiumoxide/latest/chromiumoxide/page/struct.Page.html)). |
| `headless_chrome` | 1.0.22, 2026-06-11 ([docs.rs](https://docs.rs/crate/headless_chrome/latest)) | Synchronous Chromium-only CDP; `Browser -> Tab -> Element` ([quick start](https://docs.rs/crate/headless_chrome/latest#quick-start)) | `Tab::wait_for_element` resolves a CSS selector to a borrowed element handle; element-local `wait_for_element*` and visibility waits are explicit ([`Element`](https://docs.rs/headless_chrome/latest/headless_chrome/browser/tab/element/struct.Element.html)). It has neither lazy locator composition nor Playwright strict/actionability semantics. |

None of these APIs exposes JavaScript's `AbortSignal`. Rust callers generally
cancel an async operation by dropping its future or placing it inside their own
`tokio::select!`; the reviewed public action option shapes expose timeouts, not
a common cancellation-token contract. `headless_chrome` is synchronous, so it
is an especially poor model for AUV cancellation. This is an API observation,
not a claim that a remote browser action can be rolled back after cancellation.

## What is worth borrowing

`playwright-rs` is direct evidence that the Playwright model translates cleanly
to ordinary Rust without an `IntoFuture` action builder:

```rust
let submit = page.get_by_role(
  AriaRole::Button,
  Some(GetByRoleOptions::default().name("Submit")),
);
submit.click(Some(ClickOptions::default())).await?;
```

The useful design choices are:

- make scope objects and locators cheap, cloneable descriptions;
- keep locator construction synchronous and I/O-free;
- make refinements return another locator;
- make one-target actions strict by default;
- put option-heavy action configuration in `Default` option structs;
- inherit context defaults, while allowing an action override;
- perform retry/actionability in the owner of the action, not at every caller;
- return typed errors with the locator description attached.

`thirtyfour` contributes two Rust-specific improvements worth considering. It
names strict resolution explicitly as `single()` instead of silently using
`first()`, and it gives queries descriptions that improve timeout errors. Its
separation between a query recipe and an explicit waiter is also honest when a
backend cannot provide Playwright-level actionability.

The `chromiumoxide` style of returning `&Self` from actions is convenient for
DOM command chaining, but it is not suitable for AUV: AUV needs a typed action
result containing delivery attempts, disturbances, and evidence. The
`fantoccini` and `headless_chrome` designs resolve selectors into potentially
stale element handles too early, so they are weaker references for OCR/AX
targets that must be observed again at action time.

## Recommended AUV Rust shape

Use the same concept hierarchy across Rust and TypeScript, while keeping Rust
syntax idiomatic:

```rust
let window = runner.window(WindowSelector::bundle_id("com.example.App"));

let submit = window.get_by_role(
  Role::Button,
  GetByRoleOptions::default().name("Continue"),
);

let result = submit
  .click(ClickOptions {
    timeout: Some(Duration::from_secs(5)),
    ..Default::default()
  })
  .await?;
```

For OCR, keep the evidence source explicit until a fallback policy is accepted:

```rust
let result = window
  .get_by_recognized_text(TextPattern::exact("Continue"))
  .click(ClickOptions::default())
  .await?;
```

The locator should store a serializable scope and query recipe, not a captured
coordinate or accessibility-node handle. `click()` should send one shared typed
operation to the Runner-side owner, which resolves, retries, checks uniqueness
and supported readiness, delivers input, and returns `ClickResult`. This avoids
duplicating behavior in Rust and TypeScript and preserves AUV's tracing and
typed `InputActionResult` seam.

Do not reuse any browser crate directly for this API. Their selectors,
protocols, element identities, actionability rules, and lifecycle owners are
browser-specific. `playwright-rs` and `thirtyfour` are design references; AUV
still needs its own Window/Display scopes, AX/OCR queries, Runner routing,
coordinate types, cancellation boundary, and evidence result.

## Deliberate gaps

- **TODO(locator-contract):** the serialized locator recipe remains deferred
  until the owner approves that core operation slice.
- **TODO(cancellation-contract):** whether Rust accepts a cancellation token in
  action options or relies on future cancellation remains deferred until the
  routed operation can state what cancellation means before and after input
  delivery.
- **TODO(actionability-policy):** readiness checks remain source-specific; an
  OCR match must not claim DOM/AX actionability without corresponding evidence.
