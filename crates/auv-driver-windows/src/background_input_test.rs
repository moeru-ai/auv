use super::native::{make_lparam, make_wheel_wparam, wheel_amount};

#[test]
fn make_lparam_packs_low_and_high_words() {
  assert_eq!(make_lparam(10, 20).0, (20 << 16) | 10);
}

#[test]
fn make_lparam_zero_extends_instead_of_sign_extends() {
  // A coordinate whose low 16 bits have the top bit set (e.g. y=40000) must
  // not corrupt the packed value the way a sign-extending cast would.
  let packed = make_lparam(0, 40_000).0;
  assert_eq!(packed, 40_000 << 16);
}

#[test]
fn make_wheel_wparam_places_delta_in_high_word() {
  assert_eq!(make_wheel_wparam(120).0, 120 << 16);
}

#[test]
fn make_wheel_wparam_preserves_negative_delta() {
  let packed = make_wheel_wparam(-120).0;
  let high_word = (packed >> 16) as u16 as i16;
  assert_eq!(high_word, -120);
}

#[test]
fn wheel_amount_scales_by_wheel_delta_unit() {
  assert_eq!(wheel_amount(1.0), 120);
  assert_eq!(wheel_amount(-0.5), -60);
}

#[test]
fn wheel_amount_treats_non_finite_delta_as_zero() {
  assert_eq!(wheel_amount(f64::NAN), 0);
  assert_eq!(wheel_amount(f64::INFINITY), 0);
}
