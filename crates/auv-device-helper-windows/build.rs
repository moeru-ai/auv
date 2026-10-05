use std::env;

fn main() {
  println!("cargo:rerun-if-changed=assets/auv-helper.ico");

  if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
    return;
  }

  // NOTICE(helper-icon-source): this ICO is generated from the shared
  // `AUV Helper.icon` artwork with Xcode 26 and `sips`.
  winresource::WindowsResource::new().set_icon("assets/auv-helper.ico").compile().expect("embed the AUV Helper Windows icon");
}
