//! External API service seams over AUV's runtime.
//!
//! API-P4 calls for a dedicated session API server boundary, separate from the
//! tool-facing `mcp` surface and the inspect viewer/server API. This module is
//! that owned subtree.
//!
//! Current contents:
//! - `session_service`: the execute-facing `SessionService` seam (summary read
//!   path + join policy today; transport handlers are a later slice).

pub mod session_service;
