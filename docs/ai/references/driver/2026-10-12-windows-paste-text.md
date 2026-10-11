# Windows paste text

Status: implemented on branch `feat/windows-paste-text`. This is a feature slice of
[#211](https://github.com/moeru-ai/auv/issues/211): the Windows paths of `input.pasteText` and
the `paste_text` action of `input.keyboard`. On 2026-10-12 the owner left the design to the
implementer, and this note records the choice.

## Scope and boundaries

The slice adds `InputApi::paste_text(PasteTextOptions)` to the Windows driver, as macOS and Linux
already have, and connects it to three callers:

- the ordered keyboard batch (`input_keyboard`), which `input.pasteText` and `input.keyboard` run
  through;
- the invoke helper `paste_text_into_active_control`;
- the local Runner's `PasteText` RPC.

The result is the shared `InputActionResult` with the `clipboard_paste` path. No new result schema
is added.

This replaces `TODO(windows-paste-batch)`, which held the action back until a paste had "a
verified consumption boundary".

Not in this slice:

- **Targeted (non-foreground) delivery.** Paste goes to the foreground control, like the other
  Windows keyboard commands (`TODO(windows-targeted-keyboard)`).
- **The `search`, `done` and `go` submit keys.** They are rejected, as `type_text` rejects them on
  Windows.
- **GDI-handle clipboard formats** (`CF_BITMAP`, `CF_METAFILEPICT`, `CF_PALETTE`,
  `CF_ENHMETAFILE`). The rich snapshot does not keep them, as before. Windows synthesizes
  `CF_BITMAP` from `CF_DIB`, so a copied image still comes back. A metafile or palette that exists
  only as a GDI handle does not.

## The decision: restore when the text has been read

A clipboard paste puts text on the clipboard, presses the paste key, and gives the user's
clipboard back. The hard part is *when* to give it back.

The application reads the clipboard when it handles the key, asynchronously. If the clipboard comes
back too soon, the application pastes the user's own clipboard, which may be a password, into the
target. The macOS driver waits a fixed 150 ms.

Windows can do better. **Delayed rendering** lets a clipboard owner promise a format without
supplying it: `SetClipboardData(format, NULL)`. Windows then sends the owner `WM_RENDERFORMAT` at
the moment some program asks for that format. That message is the consumption boundary the TODO
asked for.

`clipboard::lend_text` and `input::paste_text` work like this:

1. **Snapshot.** Take a snapshot of every memory-backed format on the clipboard (`snapshot_rich`).
   Note the foreground window's process.
2. **Promise the text.** A dedicated thread creates a message-only window, makes it the clipboard
   owner, and promises `CF_UNICODETEXT`. It also sets three registered formats that keep the text
   out of clipboard monitors, clipboard history and cloud clipboard:
   `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory = 0` and
   `CanUploadToCloudClipboard = 0`. The text is only passing through.
3. **Paste.** Press Ctrl+A and Backspace when `replace_existing` is set, then Ctrl+V.
4. **Wait for the first request,** up to 2 s. In `WM_RENDERFORMAT` the window hands over the text
   and records who asked. The reader is the process of the window that has the clipboard open
   (`GetOpenClipboardWindow`). It is unknown when the reader opened the clipboard without a window.
5. **Decide when the clipboard comes back:**
   - **Read by the foreground process:** give the clipboard back at once.
   - **Read first by another process, or by an unknown reader:** hold the text for 500 ms, then
     give the clipboard back. Once the text has been handed over, Windows serves it without asking
     again, so the foreground application's own read can no longer be seen.
   - **Not read within 2 s:** give the clipboard back and fail with "the foreground application
     did not read the pasted text within 2000 ms; the clipboard was restored". Nothing was pasted.
6. **Restore.** Give the user's clipboard back (`restore_rich`), then close the owner window.
   Restoring first means the window no longer owns the clipboard when it closes, so it is not asked
   to leave the text behind.
7. **Submit and settle.** Press the submit key, if any, and wait for `settle`.

The result is `verified: false`. A request for the clipboard shows that the text was consumed, not
that it landed in the control (`NOTICE(input-delivery-verification)`). The fact goes in the
attempt's message instead, for example "the foreground application read the pasted text 1 ms after
Ctrl+V". `clipboard_disturbance` is `temporary`, and pastes in one process are serialized.

### Trade-offs

- **An application that asks after 2 s gets the user's clipboard back instead of the text.** That
  is the risk the fixed wait on macOS takes after 150 ms; here it applies only past the bound, and
  the paste is reported as failed. Holding the user's clipboard for longer has its own cost (`NOTICE`
  at `PASTE_READ_TIMEOUT`).
- **A clipboard listener can hold the clipboard open at any moment.** On this machine VirtualBox's
  shared clipboard (`VBoxHeadless.exe`) opens it on every change. Opening the clipboard is retried
  everywhere, including in `WM_RENDERALLFORMATS`: a single attempt failed there with access denied
  and lost the text (`NOTICE` at the handler, and a regression test). The listener did not request
  the lent text.
- **Restoring the user's clipboard is itself a clipboard change.** Clipboard history may record it
  again. This was not checked.

## Tests

Live tests run against the real clipboard and give it back, as the existing clipboard tests do
(`clipboard_test.rs`):

- `lent_text_is_handed_to_its_first_reader_and_the_read_is_reported`
- `lent_text_nobody_reads_reports_no_read`. Nothing on this machine requested lent text uninvited,
  VirtualBox included.
- `lent_text_is_kept_out_of_clipboard_history_cloud_and_monitors`
- `a_loan_dropped_while_it_owns_the_clipboard_leaves_the_text`. This is the regression test for the
  listener finding above.

The restore decision has plain unit tests (`input_test.rs`): read by the foreground process; read
first by another process or an unknown reader, or with no foreground window; and not read at all.

## Live evidence

Host: Windows 11 Pro 10.0.29648, 2560x1440 at 100%, one display, on 2026-10-12. The `foreground`
eval scenario now pastes into its receiver's text box instead of expecting a refusal.

```sh
cargo build -p auv-cli
just eval windows foreground --output <fresh dir> --auv target/debug/auv.exe
just eval windows foreground --keep-ime --output <fresh dir> --auv target/debug/auv.exe
```

How each paste was checked:

- Before the paste steps, the harness puts its own text on the clipboard.
- After each paste, it checks that the text box gained the pasted text and that the clipboard holds
  the harness's text again.
- At the end it gives the user's own clipboard back, every format, and checks that the text is what
  it was.
- The records hold only these yes/no facts, never clipboard contents.

Evidence: [`evidence/2026-10-12-windows-paste-text/`](evidence/2026-10-12-windows-paste-text/).

| Run | Result |
| --- | --- |
| `foreground-paste-1.jsonl` to `-3.jsonl` | PASS. Both pastes (`input.pasteText ij`, then `input.keyboard` with `paste_text kl`) landed, giving `auv typedaBCdddEfghijkl`. Each was read by the foreground application 1 ms after Ctrl+V and took 43 to 59 ms through the whole `auv` process. The clipboard came back after each paste, and the user's clipboard came back at the end. 5 quiet commands held in every run. |
| `foreground-paste-keep-ime-composition-unread.jsonl` | Both pastes failed with "did not read the pasted text within 2000 ms; the clipboard was restored". The text box was unchanged and the clipboard came back. See the finding below. The run fails because the scenario still judged paste completion in this mode. |
| `foreground-paste-keep-ime.jsonl` | The same outcome, with paste completion recorded rather than judged in `--keep-ime` mode: PASS. |

### Finding: an uncommitted IME composition keeps Ctrl+V from pasting

In the `--keep-ime` runs, the key steps before the paste had left an uncommitted composition in
Microsoft Pinyin (`aBCdddEf`; see the
[key-press finding](../invoke-cli/2026-10-12-windows-screen-and-foreground-commands.md#key-presses-and-an-active-ime)).

- Ctrl+V did not reach the text box as a paste, and nothing requested the clipboard.
- The composition was committed when the window lost the foreground. No late paste of anyone's
  clipboard followed.

A fixed-delay paste would have reported success here with nothing pasted. This one reported the
failure.

Not evidenced: whether an active IME with no pending composition lets Ctrl+V through. This is the
common case of a user with Pinyin on. The scenario only reaches the paste after the key steps.

## Not evidenced

- Applications other than the harness receiver's classic edit control. This includes rich editors
  that ask for other formats first, and programs that open the clipboard without a window (they
  take the 500 ms hold).
- A reader slower than 2 s.
- Clipboard history (Win+V) and cloud clipboard behavior. The exclusion formats are set, as the
  test asserts, but their effect was not observed.
- Pasting into a window of a higher integrity level (UIPI).
- The selected Runner path. Display scaling and displays do not apply.

## Validation

```sh
cargo fmt --check
cargo clippy -p auv-driver-windows -p auv-cli-invoke -p auv-base-evals --all-targets
cargo test --workspace --locked --no-fail-fast
cargo run --quiet -- invoke --help
git diff --check
just eval windows foreground [--keep-ime] --output ... --auv target/debug/auv.exe
```
