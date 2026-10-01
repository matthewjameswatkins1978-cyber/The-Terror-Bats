//! Test probe: ignores graceful termination (Unix `SIGTERM`), then hangs.
//! The supervisor must still terminate it via forced tree kill. Not product
//! surface; exercised by `tests/supervisor.rs`.
fn main() {
    #[cfg(unix)]
    // SAFETY: installing SIG_IGN for SIGTERM is async-signal-safe and the
    // handler value is a valid constant. Unix-only test probe.
    unsafe {
        libc::signal(libc::SIGTERM, libc::SIG_IGN);
    }
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
