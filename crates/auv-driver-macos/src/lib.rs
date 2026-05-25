mod driver;

// TODO(driver-crates): This is a temporary compatibility surface for the root
// crate while legacy macOS command code is moved behind typed session APIs.
#[doc(hidden)]
pub mod native;

pub use driver::MacosDriver;
