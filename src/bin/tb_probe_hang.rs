//! Test probe: hangs until killed. Not product surface.
fn main() {
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
