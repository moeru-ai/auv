//! Occlusion reasoning based on calibrated metric depth map.

use serde::{Deserialize, Serialize};

/// Metric depth map representation where each pixel value is in meters.
#[derive(Clone, Debug, PartialEq)]
pub struct MetricDepthMap {
  pub data: Vec<f32>,
  pub width: usize,
  pub height: usize,
}

impl MetricDepthMap {
  pub fn new(data: Vec<f32>, width: usize, height: usize) -> Self {
    Self {
      data,
      width,
      height,
    }
  }

  pub fn depth_at(&self, x: usize, y: usize) -> Option<f32> {
    if x < self.width && y < self.height {
      Some(self.data[y * self.width + x])
    } else {
      None
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OcclusionVerdict {
  Visible,
  Occluded,
  Unknown,
}

/// 前向遮挡检查。
/// projected: landmark 在当前帧的 2D 投影位置（像素坐标 x, y）
/// landmark_distance_m: landmark 到相机的理论距离（米，标定后）
/// depth_map: 当前帧的米制深度图（必须已标定；未标定传 None）
/// tolerance_m: 容差，默认 1.0（覆盖深度模型噪声）
pub fn check_occlusion(
  projected: (f32, f32),
  landmark_distance_m: f64,
  depth_map: Option<&MetricDepthMap>,
  tolerance_m: f64,
) -> OcclusionVerdict {
  let Some(depth_map) = depth_map else {
    return OcclusionVerdict::Unknown;
  };

  let px = projected.0;
  let py = projected.1;

  if px < 0.0 || py < 0.0 || px >= depth_map.width as f32 || py >= depth_map.height as f32 {
    return OcclusionVerdict::Unknown;
  }

  let center_x = px.round() as isize;
  let center_y = py.round() as isize;

  let mut window = Vec::with_capacity(9);
  for dy in -1..=1 {
    for dx in -1..=1 {
      let x = center_x + dx;
      let y = center_y + dy;
      if x >= 0 && (x as usize) < depth_map.width && y >= 0 && (y as usize) < depth_map.height {
        let d = depth_map.data[(y as usize) * depth_map.width + (x as usize)];
        if d.is_finite() && d > 0.0 {
          window.push(d);
        }
      }
    }
  }

  if window.is_empty() {
    return OcclusionVerdict::Unknown;
  }

  window.sort_by(|a, b| a.total_cmp(b));
  let d_obs = window[window.len() / 2] as f64;

  if landmark_distance_m > d_obs + tolerance_m {
    OcclusionVerdict::Occluded
  } else {
    OcclusionVerdict::Visible
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_occluded_when_landmark_further_than_depth_map() {
    // 10x10 depth map with surface at 5.0m
    let depth_map = MetricDepthMap::new(vec![5.0f32; 100], 10, 10);
    // Landmark theoretical distance is 20.0m (behind the 5m wall)
    let verdict = check_occlusion((5.0, 5.0), 20.0, Some(&depth_map), 1.0);
    assert_eq!(verdict, OcclusionVerdict::Occluded);
  }

  #[test]
  fn test_visible_when_landmark_closer_than_depth_map() {
    // 10x10 depth map with surface at 5.0m
    let depth_map = MetricDepthMap::new(vec![5.0f32; 100], 10, 10);
    // Landmark theoretical distance is 4.0m (in front of the 5m surface)
    let verdict = check_occlusion((5.0, 5.0), 4.0, Some(&depth_map), 1.0);
    assert_eq!(verdict, OcclusionVerdict::Visible);
  }

  #[test]
  fn test_unknown_when_no_depth_map() {
    let verdict = check_occlusion((5.0, 5.0), 10.0, None, 1.0);
    assert_eq!(verdict, OcclusionVerdict::Unknown);
  }

  #[test]
  fn test_unknown_when_projected_out_of_bounds() {
    let depth_map = MetricDepthMap::new(vec![5.0f32; 100], 10, 10);
    let v_neg = check_occlusion((-1.0, 5.0), 10.0, Some(&depth_map), 1.0);
    assert_eq!(v_neg, OcclusionVerdict::Unknown);

    let v_over = check_occlusion((10.0, 5.0), 10.0, Some(&depth_map), 1.0);
    assert_eq!(v_over, OcclusionVerdict::Unknown);
  }

  #[test]
  fn test_tolerance_boundary_behavior() {
    let depth_map = MetricDepthMap::new(vec![5.0f32; 100], 10, 10);
    // Landmark at 5.5m with tolerance 1.0m: 5.5 <= 5.0 + 1.0 -> Visible
    let v1 = check_occlusion((5.0, 5.0), 5.5, Some(&depth_map), 1.0);
    assert_eq!(v1, OcclusionVerdict::Visible);

    // Landmark at 5.5m with tolerance 0.2m: 5.5 > 5.0 + 0.2 -> Occluded
    let v2 = check_occlusion((5.0, 5.0), 5.5, Some(&depth_map), 0.2);
    assert_eq!(v2, OcclusionVerdict::Occluded);
  }
}
