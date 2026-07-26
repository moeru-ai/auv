use super::*;

#[test]
fn normalize_absolute_maps_axis_endpoints() {
  // A 1920-wide desktop starting at origin 0 maps x=0 -> 0 and x=1919 -> 65535.
  assert_eq!(normalize_absolute(0.0, 0, 1920), 0);
  assert_eq!(normalize_absolute(1919.0, 0, 1920), 65535);
}

#[test]
fn normalize_absolute_offsets_by_virtual_origin() {
  // A secondary monitor starting at x=-1920: its left edge maps to 0.
  assert_eq!(normalize_absolute(-1920.0, -1920, 1920), 0);
}

#[test]
fn normalize_absolute_clamps_out_of_range_and_handles_degenerate_extent() {
  assert_eq!(normalize_absolute(5000.0, 0, 1920), 65535);
  assert_eq!(normalize_absolute(-50.0, 0, 1920), 0);
  assert_eq!(normalize_absolute(10.0, 0, 1), 0);
}

#[test]
fn parse_key_chord_reads_special_keys_case_insensitively() {
  assert_eq!(
    parse_key_chord("Return").unwrap(),
    KeyChord {
      modifiers: vec![],
      key: vk::RETURN,
    }
  );
  assert_eq!(
    parse_key_chord("esc").unwrap(),
    KeyChord {
      modifiers: vec![],
      key: vk::ESCAPE,
    }
  );
}

#[test]
fn parse_key_chord_reads_single_alphanumeric() {
  assert_eq!(parse_key_chord("a").unwrap().key, u16::from(b'A'));
  assert_eq!(parse_key_chord("7").unwrap().key, u16::from(b'7'));
}

#[test]
fn parse_key_chord_reads_shortcut_with_modifiers() {
  let chord = parse_key_chord("ctrl+shift+p").unwrap();

  assert_eq!(chord.modifiers, vec![vk::CONTROL, vk::SHIFT]);
  assert_eq!(chord.key, u16::from(b'P'));
}

#[test]
fn parse_key_chord_deduplicates_modifiers() {
  let chord = parse_key_chord("ctrl+control+f").unwrap();

  assert_eq!(chord.modifiers, vec![vk::CONTROL]);
  assert_eq!(chord.key, u16::from(b'F'));
}

#[test]
fn parse_key_chord_rejects_empty_and_unknown() {
  assert!(parse_key_chord("   ").is_err());
  assert!(parse_key_chord("ctrl+").is_err());
  assert!(parse_key_chord("ctrl+@").is_err());
  assert!(parse_key_chord("nope").is_err());
}

#[test]
fn text_submit_virtual_key_supports_return_only() {
  assert_eq!(text_submit_virtual_key(TextSubmit::No).unwrap(), None);
  assert_eq!(text_submit_virtual_key(TextSubmit::Return).unwrap(), Some(vk::RETURN));
  assert!(text_submit_virtual_key(TextSubmit::Search).is_err());
}
