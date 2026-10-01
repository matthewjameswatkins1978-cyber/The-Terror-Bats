//! One authoritative JCS-safe integer boundary for Bat Spec v1.
//!
//! Canonical JSON number semantics use IEEE-754 double precision, so two
//! distinct integers outside ±(2^53 − 1) could canonicalise to the same
//! number and share a content hash. Every integer that reaches the parsed
//! semantic document as an integer must therefore fit this range; larger
//! exact values must be encoded as strings.
//!
//! Out-of-range values are rejected before canonicalisation — never silently
//! converted, rounded, or stringified. No big-integer support in v1.

/// Inclusive upper bound: 2^53 − 1.
pub const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
/// Inclusive lower bound: −(2^53 − 1).
pub const MIN_SAFE_INTEGER: i64 = -9_007_199_254_740_991;

/// True when an `i64` value is exactly representable under JCS numerics.
pub fn is_safe_i64(value: i64) -> bool {
    (MIN_SAFE_INTEGER..=MAX_SAFE_INTEGER).contains(&value)
}

/// True when a `u64` value is exactly representable under JCS numerics.
pub fn is_safe_u64(value: u64) -> bool {
    value <= MAX_SAFE_INTEGER as u64
}

/// Canonical rejection message for a bare integer value.
pub fn unsafe_message(value: impl std::fmt::Display) -> String {
    format!(
        "integer value {value} exceeds the Bat Spec v1 JCS-safe range \
         ({MIN_SAFE_INTEGER}..={MAX_SAFE_INTEGER}); encode larger exact integers as strings"
    )
}
