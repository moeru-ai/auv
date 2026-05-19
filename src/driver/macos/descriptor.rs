use super::*;

pub(crate) fn driver_descriptor() -> DriverDescriptor {
  DriverDescriptor {
    id: "macos.observe",
    summary: "Observation-first desktop donor primitives extracted into the shared AUV driver protocol.",
    capabilities: &[
      "observe.screenshot",
      "observe.windows",
      "observe.ax-tree",
      "observe.permissions",
      "observe.displays",
      "observe.identify-point",
      "observe.project-screenshot-point",
      "observe.coordinate-readiness",
      "observe.screen-text",
      "observe.wait-screen-text",
      "observe.screen-rows",
      "observe.wait-screen-rows",
      "observe.image-text",
      "observe.ax-text",
      "control.activate-app",
      "control.focus-text-input",
      "control.press-button",
      "control.type-text",
      "control.paste-text-preserve-clipboard",
      "control.press-key",
      "control.click-point",
      "control.click-window-point",
      "control.click-screen-text",
      "control.click-screen-row",
      "control.scroll-point",
    ],
    donor_boundary: "Borrow host observation primitives from AIRI, but keep MCP tools, action executors, approval queues, and workflow shells out of AUV core.",
  }
}
