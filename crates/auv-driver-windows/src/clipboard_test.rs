use std::sync::Mutex;

use super::*;

// The Win32 clipboard is a single system-wide resource (OpenClipboard is
// exclusive per thread/task), so these live smoke tests must not run
// concurrently with each other or they intermittently fail with spurious
// "clipboard not open" errors from cargo test's parallel test threads.
static CLIPBOARD_TEST_LOCK: Mutex<()> = Mutex::new(());

// Live smoke test against the real Win32 clipboard. It saves and restores the
// user's existing clipboard text so the roundtrip leaves the clipboard
// unchanged.
#[test]
fn set_then_snapshot_roundtrips_and_restores_original() {
  let _lock = CLIPBOARD_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

  let original = snapshot().expect("snapshot original clipboard");

  let sentinel = "auv clipboard roundtrip \u{2713}";
  set_text(sentinel).expect("set clipboard text");
  assert_eq!(snapshot().expect("snapshot sentinel"), sentinel);

  restore(&original).expect("restore original clipboard");
  assert_eq!(snapshot().expect("snapshot restored"), original);
}

// Live smoke test against the real Win32 clipboard. Captures whatever formats
// are already present (usually just text left by the prior test, but real
// multi-format content such as an HDROP or DIB works the same way), installs
// distinct text, then restores the original snapshot and asserts the text
// format round-trips through the raw-bytes path rather than `restore`'s
// text-only path.
#[test]
fn snapshot_rich_then_restore_rich_roundtrips_captured_formats() {
  let _lock = CLIPBOARD_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

  let original_text = snapshot().expect("snapshot original text before rich capture");
  let original = snapshot_rich().expect("snapshot_rich original clipboard");

  let sentinel = "auv clipboard rich roundtrip \u{2713}";
  set_text(sentinel).expect("set clipboard text");
  assert_eq!(snapshot().expect("snapshot sentinel"), sentinel);

  restore_rich(&original).expect("restore_rich original clipboard");
  assert_eq!(snapshot().expect("snapshot restored via restore_rich"), original_text);
}

// Live smoke test against the real Win32 clipboard.
//
// ROOT CAUSE:
//
// If a clipboard format's GlobalSize payload was huge (e.g. a large image or
// custom format), snapshot_rich copied it into a Vec<u8> with no size cap,
// risking extreme memory use or an OOM-abort inside to_vec().
//
// Before the fix, an oversized format was copied unconditionally.
// The fix rejects a format whose payload exceeds a fixed per-format cap with
// a clear backend error before any matching-size allocation happens.
#[test]
fn snapshot_rich_rejects_a_format_over_the_size_cap() {
  let _lock = CLIPBOARD_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

  let original = snapshot().expect("snapshot original text before oversized capture");

  // "a".repeat(70 MiB) encodes to ~140 MiB of UTF-16 as CF_UNICODETEXT, well
  // over the 64 MiB per-format cap, so this stays robust to minor cap tuning.
  let oversized = "a".repeat(70 * 1024 * 1024);
  set_text(&oversized).expect("set oversized clipboard text");

  let error = snapshot_rich().expect_err("snapshot_rich must reject a payload over the per-format size cap");
  assert!(error.to_string().contains("exceeds"), "error should mention the size limit: {error}");

  restore(&original).expect("restore original clipboard");
}
