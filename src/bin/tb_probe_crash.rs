//! Test probe: dies abnormally (Unix signal death; Windows nonzero exit).
//! Not product surface; exercised by `tests/supervisor.rs`.
fn main() {
    std::process::abort();
}
