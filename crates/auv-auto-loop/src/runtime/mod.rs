pub mod executor;
pub mod fake;

pub use executor::{ExecutionResult, OperationExecutor, RuntimeEnvironment, RuntimeExecutor};
pub use fake::FakeOperationExecutor;
