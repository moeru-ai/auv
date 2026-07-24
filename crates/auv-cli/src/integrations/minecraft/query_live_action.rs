use auv_driver::{InputActionResult, geometry::WindowPoint};

/// Delivers direct Minecraft window clicks for live projection workflows.
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

  /// Delivers one typed window click.
  pub fn click(&self, window_point: WindowPoint) -> Result<InputActionResult, String> {
    let session = auv_driver::open_local().map_err(|error| error.to_string())?;
    let window = session
      .window()
      .resolve(auv_driver::WindowSelector {
        app: Some(auv_driver::App::bundle_id(self.target_app.clone())),
        title: Some(auv_driver::TextMatcher::Contains(self.target_title.clone())),
        main_visible: true,
      })
      .map_err(|error| error.to_string())?;
    let action = session.window().click(&window, window_point, auv_driver::ClickOptions::default()).map_err(|error| error.to_string())?;
    Ok(action)
  }
}
