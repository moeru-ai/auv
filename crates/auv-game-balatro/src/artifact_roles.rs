//! Durable Balatro inspect/run artifact role names.
//!
//! NOTICE(inspect-composition / S3a): Roles live in the donor crate so readers
//! can graduate out of `auv-cli` without copying string constants.

pub const BALATRO_CARD_DETECTION_SEMANTIC_ROLE: &str = "balatro-card-detection-semantic";
pub const BALATRO_CARD_DETECTION_SEMANTIC_INSPECT_ROLE: &str = "balatro-card-detection-semantic-inspect";
pub const BALATRO_CARD_DETECTION_SPATIAL_QUERY_ROLE: &str = "balatro-card-detection-spatial-query";
pub const BALATRO_CARD_DETECTION_SPATIAL_QUERY_INSPECT_ROLE: &str = "balatro-card-detection-spatial-query-inspect";
pub const BALATRO_CARD_DETECTION_EVAL_WITNESS_ROLE: &str = "balatro-card-detection-eval-witness";
pub const BALATRO_CARD_DETECTION_EVAL_WITNESS_INSPECT_ROLE: &str = "balatro-card-detection-eval-witness-inspect";
pub const BALATRO_CARD_DETECTION_QUALITY_ROLE: &str = "balatro-card-detection-quality";
pub const BALATRO_CARD_DETECTION_QUALITY_INSPECT_ROLE: &str = "balatro-card-detection-quality-inspect";
