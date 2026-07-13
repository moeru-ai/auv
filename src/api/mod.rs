//! External API service seams over AUV's runtime.
//!
//! The session API server is separate from the tool-facing `mcp` surface and
//! the inspect viewer/server API.
//!
//! Current contents:
//! - `session_service`: execute-facing application, gRPC, server, and summary
//!   read/write boundaries.

pub mod session_service;
