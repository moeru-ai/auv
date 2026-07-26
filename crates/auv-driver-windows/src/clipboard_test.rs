use super::*;

// Live smoke test against the real Win32 clipboard. It saves and restores the
// user's existing clipboard text so the roundtrip leaves the clipboard
// unchanged.
#[test]
fn set_then_snapshot_roundtrips_and_restores_original() {
  let original = snapshot().expect("snapshot original clipboard");

  let sentinel = "auv clipboard roundtrip \u{2713}";
  set_text(sentinel).expect("set clipboard text");
  assert_eq!(snapshot().expect("snapshot sentinel"), sentinel);

  restore(&original).expect("restore original clipboard");
  assert_eq!(snapshot().expect("snapshot restored"), original);
}
