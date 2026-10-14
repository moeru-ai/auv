use super::*;

fn atoms() -> Atoms {
  Atoms {
    CLIPBOARD: 100,
    CLIPBOARD_MANAGER: 101,
    TARGETS: 102,
    TIMESTAMP: 103,
    MULTIPLE: 104,
    UTF8_STRING: 105,
    TEXT: 106,
    TEXT_PLAIN_UTF8: 107,
    INCR: 108,
    AUV_SELECTION: 109,
    AUV_TIMESTAMP: 110,
    AUV_MARKER: 111,
    _NET_SUPPORTING_WM_CHECK: 112,
  }
}

fn served(text: &str) -> Served {
  Served {
    text: text.into(),
    timestamp: 5_000,
  }
}

// ICCCM's predefined STRING atom (31) needs no interning.
const STRING: Atom = 31;

#[test]
fn targets_advertise_string_only_for_latin1_text() {
  let atoms = atoms();
  let Answer::Atoms(targets) = answer(&atoms, Some(&served("héllo")), atoms.TARGETS, CURRENT_TIME, 1024) else {
    panic!("TARGETS must list atoms");
  };
  assert_eq!(
    targets,
    [
      atoms.TARGETS,
      atoms.TIMESTAMP,
      atoms.UTF8_STRING,
      atoms.TEXT,
      atoms.TEXT_PLAIN_UTF8,
      STRING
    ]
  );

  let Answer::Atoms(targets) = answer(&atoms, Some(&served("héllo 世界")), atoms.TARGETS, CURRENT_TIME, 1024) else {
    panic!("TARGETS must list atoms");
  };
  assert!(!targets.contains(&STRING));
  assert_eq!(answer(&atoms, Some(&served("世界")), STRING, CURRENT_TIME, 1024), Answer::Refuse);
}

#[test]
fn text_targets_return_utf8_or_latin1_bytes() {
  let atoms = atoms();
  let text = served("héllo 世界");
  for target in [atoms.UTF8_STRING, atoms.TEXT] {
    assert_eq!(
      answer(&atoms, Some(&text), target, CURRENT_TIME, 1024),
      Answer::Text {
        type_: atoms.UTF8_STRING,
        bytes: "héllo 世界".as_bytes().to_vec()
      }
    );
  }
  assert_eq!(
    answer(&atoms, Some(&text), atoms.TEXT_PLAIN_UTF8, CURRENT_TIME, 1024),
    Answer::Text {
      type_: atoms.TEXT_PLAIN_UTF8,
      bytes: "héllo 世界".as_bytes().to_vec()
    }
  );
  assert_eq!(
    answer(&atoms, Some(&served("héllo")), STRING, CURRENT_TIME, 1024),
    Answer::Text {
      type_: STRING,
      bytes: vec![b'h', 0xe9, b'l', b'l', b'o']
    }
  );
  assert_eq!(answer(&atoms, Some(&text), atoms.TIMESTAMP, CURRENT_TIME, 1024), Answer::Timestamp(5_000));
}

#[test]
fn unsupported_stale_and_unowned_requests_are_refused() {
  let atoms = atoms();
  let text = served("text");
  assert_eq!(answer(&atoms, Some(&text), atoms.MULTIPLE, CURRENT_TIME, 1024), Answer::Refuse);
  assert_eq!(answer(&atoms, Some(&text), 999, CURRENT_TIME, 1024), Answer::Refuse);
  // ICCCM 2.2: a request timestamped before ownership predates our content.
  assert_eq!(answer(&atoms, Some(&text), atoms.UTF8_STRING, 4_999, 1024), Answer::Refuse);
  assert!(matches!(answer(&atoms, Some(&text), atoms.UTF8_STRING, 5_000, 1024), Answer::Text { .. }));
  assert_eq!(answer(&atoms, None, atoms.TARGETS, CURRENT_TIME, 1024), Answer::Refuse);
}

#[test]
fn text_above_one_request_is_refused_and_rejected_before_input() {
  let atoms = atoms();
  assert_eq!(answer(&atoms, Some(&served("12345")), atoms.UTF8_STRING, CURRENT_TIME, 4), Answer::Refuse);
  assert!(matches!(answer(&atoms, Some(&served("1234")), atoms.UTF8_STRING, CURRENT_TIME, 4), Answer::Text { .. }));
  assert!(check_size("1234", 4).is_ok());
  // The limit counts UTF-8 bytes, not characters.
  assert!(matches!(check_size("世界", 4), Err(auv_driver_common::DriverError::InvalidInput { .. })));
}

#[test]
fn non_text_owner_targets_mark_the_snapshot_rich() {
  for name in [
    "TARGETS",
    "UTF8_STRING",
    "STRING",
    "TEXT",
    "text/plain",
    "text/plain;charset=utf-8",
    "SAVE_TARGETS",
  ] {
    assert!(is_text_or_meta(name), "{name}");
  }
  for name in [
    "text/html",
    "image/png",
    "x-special/gnome-copied-files",
    "chromium/x-source-url",
  ] {
    assert!(!is_text_or_meta(name), "{name}");
  }
}
