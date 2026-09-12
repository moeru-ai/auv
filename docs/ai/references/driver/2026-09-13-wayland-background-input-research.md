# Wayland background input: research and accepted boundary

Date: 2026-09-13. Classification: docs-only research and decision record.

AUV keeps its existing background/foreground input policies. Caller code owns
coordination between automation and the person using the desktop. This review
does not introduce public agent/user claims or seats. The approved implementation
work is click modifiers, Linux foreground keyboard integration, Portal identity
and authorization reuse, and local Runner lifecycle. A controlled compositor is
an option for a future, separately approved slice.

## Evidence and implementation status

The four September 9 notes are historical source reviews, preserved with their
original revisions and candidate IDs:

- [Framework comparison](2026-09-09-computer-use-framework-comparison-note.md).
- [Code and upstream review](2026-09-09-computer-use-code-and-upstream-review.md).
- [Improvement candidates](2026-09-09-computer-use-improvement-candidates.md).
- [Background input, AX, and media review](2026-09-09-background-ax-and-media-gap-review.md).

Their tables describe AUV at `bae42bf9905614b19347566d5d41b3a9998e8a35`,
not the following implementation branches. In particular, BG-1's modifier gap
and BG-8's Linux keyboard frontend gap have subsequent implementation work:

| Change | Reviewable implementation | Evidence level and boundary |
| --- | --- | --- |
| ClickModifiers across driver, Rust API, invoke, Runner, Proto, and JS SDK; repeated `--modifiers cmd --modifiers shift` | [PR #178](https://github.com/moeru-ai/auv/pull/178), head `5052c7b42c1e22f23866c5d028dcff7391dbeb11` | Contract tests and macOS receiver evidence; [contract and platform limits][click-evidence]. Mouse button selection and arbitrary physical keyboard keycodes are separate contracts. |
| ashpd Portal clients, stable app identity, restore-token rotation, explicit uinput, daemon health `id`, worker reuse, Linux keyboard batches through CLI/Runner | [PR #179](https://github.com/moeru-ai/auv/pull/179), head `e39f9f141f5de72ae3b39d65f71f21b9501da08c` | Source/tests plus logged-in GNOME GTK receipts and separate headless capture evidence; [authorization/lifecycle][portal-evidence] and [reproduction/evidence][linux-evidence]. Stacked on #178. |
| Wayland background alternatives in this note | Pinned upstream source and protocol documentation | No AUV live validation of CUA's compositor, its Hyprland plugin, wprs, or transient seats. Upstream test reports remain upstream evidence. |

At the recorded heads, both implementation PRs' GitHub Rust/JS checks on Linux,
macOS, and Windows and vendored-Protobuf checks passed. This is CI evidence;
it does not substitute for native interactive receiver tests.

## Three different requirements

| Requirement | Mechanism | What it does not establish |
| --- | --- | --- |
| Reuse authorization across process restarts | Portal app identity, persistence request, restore-token storage/rotation | An arbitrary window recipient or an independent input focus |
| Operate an existing background application on the person's desktop | Semantic application actions or a compositor-controlled target/seat route | Compatibility with every application, compositor, or window state |
| Run unattended without interfering with the person's desktop | Separate headless/nested compositor and applications launched into it | Delivery to arbitrary applications already running on the host compositor |

A token is not a window address. RemoteDesktop requests devices and authorizes
input for a session; its persistence mode can request permission until revoked.
Restore tokens are single-use and must be replaced with the token returned by
a successful restoration. The backend may reject restoration and require user
interaction. Stable `ai.moeru.auv` identity and durable storage make rebuilding
or restarting AUV compatible with reuse; they do not promise perpetual consent
or compositor-reboot behavior that has not been tested. See the official
[RemoteDesktop contract][remote-desktop] and [AUV authorization evidence][portal-evidence].

Starting a daemon in a logged-in graphical session supplies access to that
session's bus, compositor, and permission environment. Reusing that process
also retains active sessions. It cannot add RemoteDesktop to a Portal backend
that does not implement the interface. SSH by itself does not create a graphical
session or authorize a desktop input device.

Sunshine's Linux [input implementation][sunshine-input], inspected at
`dd7a1f796e69283a42663630ecd49b174b070778`, uses a libvirtualhid runtime.
That source has evolved from the earlier libevdev/uinput discussion; ongoing
remote input is not proof of a more durable Portal token. AUV's explicit uinput
route requires device access and delivers foreground input. The Linux dependency is
target-scoped; this choice does not add evdev to the Windows build. No antivirus
compatibility claim follows from that build boundary.

## Existing-desktop Wayland options

Wayland compositors own input routing and seat focus. A compositor can implement
targeted delivery; the missing piece is a portable, commonly deployed client
interface with the required behavior and application coverage. The source review
found several approaches, rather than a single universal solution:

| Approach | Target/focus behavior | Shipping and evidence boundary |
| --- | --- | --- |
| AT-SPI actions and value operations | Address an accessible object semantically; an app may still change focus as a side effect | Depends on exposed actions and app behavior. CUA uses guarded semantic routes; AUV's existing focus/select calls do not establish a general no-disturbance guarantee. |
| Portal/libei or uinput | Input follows compositor routing and current seat focus | Useful for foreground automation and unattended dedicated sessions; no general background window address. |
| Hyprland target keyboard dispatcher | Resolves a target, temporarily changes keyboard focus, sends input, restores focus | Compositor-specific. Restoring focus does not mean the client observed no enter/leave events. [Pinned implementation][hypr-actions]. |
| Gabriel-Kahen's Hyprland pointer plugin | Temporarily assigns pointer focus to a target surface, sends click/scroll/drag events, then restores it | Avoids moving the physical cursor in this route, but transient pointer focus and hover events remain observable. Exact Hyprland ABI required; source-reviewed only. [Implementation][target-pointer]. |
| CUA experimental Hyprland seats | Independent compositor seats and exact-target grants with conflict checks | Optional, unreleased experiment with narrow app/keymap qualification; details below. |
| `ext-transient-seat-v1` | Creates a temporary independent seat; virtual keyboard/pointer protocols supply input devices | No target-window operation. Availability and application handling require separate checks. |

Hyprland is one Wayland compositor, alongside Mutter, KWin, Sway, Jay, and
others. A Hyprland plugin is not a cross-Wayland plugin. The dispatcher inspected
at `3a37b75f651b7f25f68e03ff6cc2b85d403cb633` calls a save/set/send/restore
keyboard-focus path. The pointer plugin inspected at
`cca71f282fe742519046f568143a00a6eb27de36` uses `PointerFocusRestore` around
its target transaction. Both are relevant alternatives, but neither proves
independent-seat behavior across desktop environments.

### Transient-seat availability

The [protocol XML][transient-xml] defines creation of a temporary seat, including
compositor refusal. It does not define application discovery, toplevel selection,
or a way to focus an arbitrary target. A client still needs compatible virtual
input protocols and a routing policy.

Implementation evidence was found in [Sway][sway-seat] and [Jay][jay-seat]; Sway
also lists the protocol in its [1.10 release notes][sway-release]. The
[Wayland Explorer matrix][seat-matrix] is a useful discovery aid, not a desktop
market-share figure or a guarantee for the user's installed version. The review's
GitHub searches found no corresponding implementation in Mutter, KWin, or
Hyprland; absence from search results is not conclusive proof of non-support.
A deployment needs a live registry probe and target-app tests before claiming
this route. No such AUV probe or delivery test was performed in this research.

### What CUA actually does

This follow-up inspected CUA at
`044448f4cf46b9777c63df0d5a07d4432674b46e`. It supersedes the earlier note's
CUA revisions only for the Linux routes discussed here.

CUA's [support record][cua-support] distinguishes deliveries from exact refusals.
For example, its X11 116-outcome report includes 75 deliveries and 41 refusals;
those numbers must not be presented as 116 successful background deliveries.
Stock Wayland raw input remains focus-bound. Its GNOME helper provides window
identity, geometry, activation, capture, and cursor information; guarded AT-SPI
handles applicable semantic background actions. That helper is not a generic
raw background input endpoint. KWin-specific support also does not establish
portable target-addressed raw input.

The optional [nested compositor patch][cua-compositor] owns a wlroots/tinywl
session and exposes a private injection socket. It resolves application identity
and addresses client keyboard resources directly. Pointer behavior has a relevant
exception: one route updates seat pointer focus to satisfy client/toolkit event
handling; other routes send to client resources directly. Its source therefore
does not justify calling every action focus-free. It also confines hit testing
to the target subtree for occluded targets. Upstream records GTK3 31/31 and
capture/scope 5/5, while the accepted full Electron run still has 10 failures
(26/36). Focused repairs are not a replacement for full qualification.

The separate [Hyprland plugin][cua-hyprland] is disabled by default and its normal
build provides discovery/status only. An opt-in candidate uses two independent
seats, target/lifetime checks, bounded grants, and cancellation/cleanup when
user focus conflicts with the target client. Its recorded qualification is
limited to native Calc `libreoffice-fresh 26.2.5-3`, Inkscape `1.4.4-6`, and the
plain `evdev`/`pc105`/`us` keymap. Chromium, Electron, and XWayland raw background
input remain unqualified. It requires the exact compositor ABI and compatible
C++ runtime; plugin replacement requires a desktop restart. Source version
`0.23.2` does not mean this candidate shipped in that released driver.

## Controlled compositor and application wrapper

[wprs architecture][wprs] is relevant to applications launched under an owned
compositor. Its Rust/Smithay server serializes Wayland objects to a client that
creates corresponding local windows and forwards events back to their owners.
It supports reconnecting to the remote session. That establishes remote window
presentation and input forwarding, not an existing automation API for injecting
into any host background window. Applications must connect to its compositor
when launched; it does not adopt arbitrary running Mutter/KWin clients.

[waymux][waymux] similarly describes a Rust headless Wayland runtime for isolated
automation sessions. [agent-sh][agent-sh] combines accessibility and desktop-specific
control; [Roadmvn][roadmvn] documents Xephyr isolation. These are distinct scopes,
and none of their README claims counts as AUV runtime validation. Peekaboo and
KWWK's native component are macOS references in the earlier research, not Linux
input backends.

For applications that AUV is allowed to launch, owning a compositor makes target
identity, input routing, and rendering controllable in one environment. This is
a plausible packaging direction, not a finding that wprs or a custom compositor
is already an out-of-the-box AUV solution. Toolkit behavior, GPU/buffer handling,
clipboard, popups, accessibility, process lifecycle, and packaging remain work.
Foreground input inside that environment must still be described as foreground
there, even when it does not disturb the host desktop.

## AUV's accepted boundary and future validation trigger

At the #179 head, Linux raw input uses Portal or explicitly selected uinput.
Window click/scroll paths reject `BackgroundOnly`; permitted paths use foreground
input. Ordered keyboard batches accept a foreground target and reject
application/window targets. AT-SPI observation/focus/select exists, but the
current result reports foreground disturbance and has no general background
behavior proof. These source boundaries are recorded in [session.rs][auv-session],
[keyboard.rs][auv-keyboard], and [accessibility.rs][auv-accessibility].

The logged-in GNOME test demonstrated modifier clicks and CLI/Runner keyboard
receipt in a dedicated GTK application. The separate ihome container demonstrated
Electron rendering and repeated unattended Portal/PipeWire capture under headless
Sway/pixman, using the image's AUV 0.0.13. It did not validate GPU rendering,
the new uinput route, or same-desktop background input. Windows desktop testing,
KDE unattended behavior, and reboot restoration remain outside that evidence.
Receipts and environment details stay with [the implementation PR][linux-evidence].

NOTICE: A custom compositor, transient-seat integration, and compositor-specific
background plugins are deferred. This round selected existing foreground and
authorization work; a new owner-approved slice must name whether it targets
existing host applications or AUV-launched applications before implementing one.
X11 remains separate research and is not implemented or qualified by this PR.

A future background slice should validate an exact app/toolkit and compositor
version with an independent receiver: target identity, down/up/modifier state,
focus and pointer observations before/during/after, user-input conflicts,
minimized/occluded states, and release after cancellation or disconnect. Restored
focus must not be reported as uninterrupted focus. Protocol acceptance and
`InputActionResult` delivery success remain separate from app-owned semantic
verification. No new shared vocabulary is introduced here.

[click-evidence]: https://github.com/moeru-ai/auv/blob/5052c7b42c1e22f23866c5d028dcff7391dbeb11/docs/ai/references/driver/2026-09-11-click-modifiers-contract.md
[portal-evidence]: https://github.com/moeru-ai/auv/blob/e39f9f141f5de72ae3b39d65f71f21b9501da08c/docs/ai/references/driver/2026-09-12-linux-portal-authorization-and-runner-reuse.md
[linux-evidence]: https://github.com/moeru-ai/auv/blob/e39f9f141f5de72ae3b39d65f71f21b9501da08c/docs/ai/references/driver/2026-09-12-uinput-and-wayland-validation.md
[remote-desktop]: https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html
[sunshine-input]: https://github.com/LizardByte/Sunshine/blob/dd7a1f796e69283a42663630ecd49b174b070778/src/platform/linux/input/virtualhid.cpp
[transient-xml]: https://github.com/wayland-mirror/wayland-protocols/blob/main/staging/ext-transient-seat/ext-transient-seat-v1.xml
[sway-seat]: https://github.com/swaywm/sway/blob/793a1f0c4702e2223bded3e01623aeb5a442a3ca/sway/input/input-manager.c
[jay-seat]: https://github.com/mahkoh/jay/blob/1dca20f4b61cd1944e860d41043e7c14c9c0f871/src/ifs/wl_seat/ext_transient_seat_manager_v1.rs
[sway-release]: https://github.com/swaywm/sway/releases/tag/1.10
[seat-matrix]: https://wayland.app/protocols/ext-transient-seat-v1
[hypr-actions]: https://github.com/hyprwm/Hyprland/blob/3a37b75f651b7f25f68e03ff6cc2b85d403cb633/src/config/shared/actions/ConfigActions.cpp
[target-pointer]: https://github.com/Gabriel-Kahen/hyprland-codex-background-computer-use/blob/cca71f282fe742519046f568143a00a6eb27de36/hyprland/target-pointer.cpp
[cua-support]: https://github.com/trycua/cua/blob/044448f4cf46b9777c63df0d5a07d4432674b46e/libs/cua-driver/docs/action-support.md
[cua-compositor]: https://github.com/trycua/cua/blob/044448f4cf46b9777c63df0d5a07d4432674b46e/nix/cua-driver/compositor/cua_compositor_patch.py
[cua-hyprland]: https://github.com/trycua/cua/blob/044448f4cf46b9777c63df0d5a07d4432674b46e/libs/cua-driver/hyprland-plugin/README.md
[wprs]: https://github.com/wayland-transpositor/wprs/blob/12b864dc5b308e63edede4714f664afa88507fce/README.md#architecture
[waymux]: https://github.com/tek-cat/waymux
[agent-sh]: https://github.com/agent-sh/computer-use-linux
[roadmvn]: https://github.com/Roadmvn/linux-computer-use/blob/main/docs/usage.md
[auv-session]: https://github.com/moeru-ai/auv/blob/e39f9f141f5de72ae3b39d65f71f21b9501da08c/crates/auv-driver-linux/src/session.rs
[auv-keyboard]: https://github.com/moeru-ai/auv/blob/e39f9f141f5de72ae3b39d65f71f21b9501da08c/crates/auv-driver-linux/src/keyboard.rs
[auv-accessibility]: https://github.com/moeru-ai/auv/blob/e39f9f141f5de72ae3b39d65f71f21b9501da08c/crates/auv-driver-linux/src/accessibility.rs
