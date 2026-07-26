use super::*;

fn valid_projection_json() -> serde_json::Value {
  serde_json::json!({
    "kind": "playfield_to_pixels",
    "scale_x": 1.0,
    "scale_y": 1.0,
    "offset_x": 0.0,
    "offset_y": 0.0,
    "match_radius_px": 20.0
  })
}

#[test]
fn split_projection_decoder_rejects_inconsistent_tagged_union_fields() {
  let path = Path::new("visual-eval.json");
  let cases = [
    serde_json::json!({"kind": "unavailable", "reason": "missing", "scale_x": 1.0}),
    {
      let mut value = valid_projection_json();
      value["reason"] = serde_json::json!("not unavailable");
      value
    },
    {
      let mut value = valid_projection_json();
      value["extra"] = serde_json::json!(true);
      value
    },
  ];

  for value in cases {
    assert!(decode_eval_projection(&value, path).is_err(), "accepted inconsistent projection {value}");
  }
}

#[test]
fn split_projection_decoder_rejects_invalid_positive_fields() {
  let path = Path::new("visual-eval.json");
  for (field, value) in [
    ("scale_x", 0.0),
    ("scale_y", -1.0),
    ("match_radius_px", 0.0),
    ("match_radius_px", -1.0),
    ("scale_x", 1.0e-100),
    ("match_radius_px", 1.0e-100),
  ] {
    let mut projection = valid_projection_json();
    projection[field] = serde_json::json!(value);
    assert!(decode_eval_projection(&projection, path).is_err(), "accepted {field}={value}");
  }
}
