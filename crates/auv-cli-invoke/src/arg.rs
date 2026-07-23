#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArgSpec {
  pub flag: &'static str,
  pub value_name: &'static str,
  pub required: bool,
  pub help: &'static str,
}

pub const TARGET: ArgSpec = ArgSpec {
  flag: "--target",
  value_name: "APP",
  required: false,
  help: "Target application identifier or name.",
};
pub const TITLE: ArgSpec = ArgSpec {
  flag: "--title",
  value_name: "TEXT",
  required: false,
  help: "Window title text used to select the capture target.",
};
pub const LABEL: ArgSpec = ArgSpec {
  flag: "--label",
  value_name: "LABEL",
  required: false,
  help: "Human-readable label for the operation.",
};
pub const QUERY: ArgSpec = ArgSpec {
  flag: "--query",
  value_name: "TEXT",
  required: true,
  help: "Text query used by the invoke command.",
};
pub const OPTIONAL_QUERY: ArgSpec = ArgSpec {
  flag: "--query",
  value_name: "TEXT",
  required: false,
  help: "Text query used by the invoke command; required unless --candidate is supplied.",
};
pub const CANDIDATE: ArgSpec = ArgSpec {
  flag: "--candidate",
  value_name: "JSON",
  required: false,
  help: "Promoted typed candidate JSON consumed instead of --query where supported.",
};
pub const TEXT: ArgSpec = ArgSpec {
  flag: "--text",
  value_name: "TEXT",
  required: true,
  help: "Text content used by the invoke command.",
};
pub const CONTENT: ArgSpec = ArgSpec {
  flag: "--content",
  value_name: "TEXT",
  required: true,
  help: "Document body text to write.",
};
pub const REPLACE: ArgSpec = ArgSpec {
  flag: "--replace",
  value_name: "BOOL",
  required: false,
  help: "Replace existing document body before paste (default true).",
};
pub const VERIFY: ArgSpec = ArgSpec {
  flag: "--verify",
  value_name: "BOOL",
  required: false,
  help: "Run AX text verification after paste (default true).",
};
pub const TARGET_TEXT: ArgSpec = ArgSpec {
  flag: "--target_text",
  value_name: "TEXT",
  required: true,
  help: "Expected AX text used by the invoke command.",
};
pub const IMAGE_PATH: ArgSpec = ArgSpec {
  flag: "--image_path",
  value_name: "PATH",
  required: true,
  help: "Existing image artifact or PNG path to inspect.",
};
pub const KEY: ArgSpec = ArgSpec {
  flag: "--key",
  value_name: "KEY",
  required: true,
  help: "Keyboard key or shortcut to press.",
};
pub const OVERLAY: ArgSpec = ArgSpec {
  flag: "--overlay",
  value_name: "BOOL",
  required: false,
  help: "Draw visual-only overlay cursor evidence where supported.",
};
pub const REGION_X: ArgSpec = ArgSpec {
  flag: "--x",
  value_name: "NUMBER",
  required: true,
  help: "Region left coordinate in the selected coordinate space.",
};
pub const REGION_Y: ArgSpec = ArgSpec {
  flag: "--y",
  value_name: "NUMBER",
  required: true,
  help: "Region top coordinate in the selected coordinate space.",
};
pub const REGION_WIDTH: ArgSpec = ArgSpec {
  flag: "--width",
  value_name: "NUMBER",
  required: true,
  help: "Region width in the selected coordinate space.",
};
pub const REGION_HEIGHT: ArgSpec = ArgSpec {
  flag: "--height",
  value_name: "NUMBER",
  required: true,
  help: "Region height in the selected coordinate space.",
};
pub const OFFSET_X: ArgSpec = ArgSpec {
  flag: "--offset_x",
  value_name: "NUMBER",
  required: false,
  help: "Absolute window-pixel X coordinate for the click target.",
};
pub const OFFSET_Y: ArgSpec = ArgSpec {
  flag: "--offset_y",
  value_name: "NUMBER",
  required: false,
  help: "Absolute window-pixel Y coordinate for the click target.",
};
pub const RELATIVE_X: ArgSpec = ArgSpec {
  flag: "--relative_x",
  value_name: "NUMBER",
  required: false,
  help: "Relative window X coordinate in 0..1 against the resolved window width.",
};
pub const RELATIVE_Y: ArgSpec = ArgSpec {
  flag: "--relative_y",
  value_name: "NUMBER",
  required: false,
  help: "Relative window Y coordinate in 0..1 against the resolved window height.",
};

pub const TARGET_ARGS: &[ArgSpec] = &[TARGET];
pub const WINDOW_ARGS: &[ArgSpec] = &[TARGET, TITLE];
pub const WINDOW_CLICK_POINT_ARGS: &[ArgSpec] = &[TARGET, TITLE, OFFSET_X, OFFSET_Y, RELATIVE_X, RELATIVE_Y];
pub const LABEL_ARGS: &[ArgSpec] = &[TARGET, LABEL];
pub const SCREEN_TEXT_ARGS: &[ArgSpec] = &[TARGET, QUERY];
pub const IMAGE_TEXT_ARGS: &[ArgSpec] = &[QUERY, IMAGE_PATH];
pub const TEXT_ARGS: &[ArgSpec] = &[TARGET, TEXT];
pub const KEY_ARGS: &[ArgSpec] = &[TARGET, KEY];
pub const QUERY_ARGS: &[ArgSpec] = &[TARGET, QUERY];
pub const WINDOW_TEXT_ARGS: &[ArgSpec] = &[TARGET, TITLE, QUERY];
pub const WINDOW_VERIFY_TEXT_ARGS: &[ArgSpec] = &[TARGET, TARGET_TEXT];
pub const QUERY_OR_CANDIDATE_ARGS: &[ArgSpec] = &[TARGET, OPTIONAL_QUERY, CANDIDATE];
pub const QUERY_OVERLAY_ARGS: &[ArgSpec] = &[TARGET, QUERY, OVERLAY];
pub const QUERY_OR_CANDIDATE_OVERLAY_ARGS: &[ArgSpec] = &[TARGET, OPTIONAL_QUERY, CANDIDATE, OVERLAY];
pub const WINDOW_QUERY_OVERLAY_ARGS: &[ArgSpec] = &[TARGET, TITLE, QUERY, OVERLAY];
pub const REGION_ARGS: &[ArgSpec] = &[
  TARGET,
  REGION_X,
  REGION_Y,
  REGION_WIDTH,
  REGION_HEIGHT,
  LABEL,
];
pub const FIXTURE_DIR: ArgSpec = ArgSpec {
  flag: "--fixture-dir",
  value_name: "PATH",
  required: true,
  help: "Directory containing a single-frame scan fixture (manifest.json + PNG).",
};
pub const SCAN_FRAME_ARGS: &[ArgSpec] = &[FIXTURE_DIR];
pub const COVERAGE_FIXTURE_DIR: ArgSpec = ArgSpec {
  flag: "--fixture-dir",
  value_name: "PATH",
  required: true,
  help: "Directory containing a coverage scenario manifest (manifest.json); frame PNGs are resolved via frame_fixture cross-reference, not stored in this directory.",
};
pub const SCAN_COVERAGE_ARGS: &[ArgSpec] = &[COVERAGE_FIXTURE_DIR];
pub const TEXTEDIT_DOCUMENT_WRITE_ARGS: &[ArgSpec] = &[CONTENT, REPLACE, VERIFY];
pub const NO_ARGS: &[ArgSpec] = &[];
