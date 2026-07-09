pub const VIEWER_HTML: &str = include_str!("../viewer/dist/index.html");

pub const VIEWER_ASSETS: &[(&str, &[u8], &str)] = &[
  ("assets/viewer.js", include_bytes!("../viewer/dist/assets/viewer.js"), "text/javascript; charset=utf-8"),
  ("assets/index.css", include_bytes!("../viewer/dist/assets/index.css"), "text/css; charset=utf-8"),
];

pub fn viewer_asset(name: &str) -> Option<(&'static [u8], &'static str)> {
  if name.is_empty() || name.contains('\\') || name.contains("..") || name.starts_with('.') {
    return None;
  }
  VIEWER_ASSETS.iter().find(|(asset_name, _, _)| *asset_name == name).map(|(_, bytes, mime)| (*bytes, *mime))
}
