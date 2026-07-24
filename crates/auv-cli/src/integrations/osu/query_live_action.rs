use auv_driver::{InputActionResult, geometry::WindowPoint};
use auv_game_osu::{VisualTruthQueryActionWiringLineage, VisualTruthQueryLiveClickExecutor};

pub const QUERY_WIRED_LIVE_ACTION_OPERATION_ID: &str = "auv.osu.visual_truth_query_wired_live_action";

pub struct DirectWindowPointClickExecutor {
  target_app: String,
  target_title: String,
}

impl DirectWindowPointClickExecutor {
  pub fn new(target_app: impl Into<String>, target_title: impl Into<String>) -> Self {
    Self {
      target_app: target_app.into(),
      target_title: target_title.into(),
    }
  }
}

impl VisualTruthQueryLiveClickExecutor for DirectWindowPointClickExecutor {
  fn attempt_click(&self, window_point: WindowPoint, _lineage: &VisualTruthQueryActionWiringLineage) -> Result<InputActionResult, String> {
    let session = auv_driver::open_local().map_err(|error| error.to_string())?;
    let window = session
      .window()
      .resolve(auv_driver::WindowSelector {
        app: Some(auv_driver::App::name(self.target_app.clone())),
        title: Some(auv_driver::TextMatcher::Contains(self.target_title.clone())),
        main_visible: true,
        ..auv_driver::WindowSelector::default()
      })
      .map_err(|error| error.to_string())?;
    let action = session.window().click(&window, window_point, auv_driver::ClickOptions::default()).map_err(|error| error.to_string())?;
    Ok(action)
  }
}
