use serde::{Deserialize, Serialize};

use crate::geometry::{CoordinateSpace, Rect};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Display {
  pub id: String,
  pub name: Option<String>,
  pub frame: Rect,
  pub coordinate_space: CoordinateSpace,
  pub scale_factor: f64,
  pub is_primary: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ObservedDisplays {
  pub displays: Vec<Display>,
}
