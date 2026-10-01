//! Test probe: sleeps, then exits with a code. Used for timeout-vs-exit
//! races. Not product surface; exercised by `tests/supervisor.rs`.
fn main() {
    let mut sleep_ms: u64 = 0;
    let mut code = 0;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--sleep-ms" => sleep_ms = args.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            "--code" => code = args.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            other => {
                eprintln!("unknown flag `{other}`");
                std::process::exit(2);
            }
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(sleep_ms));
    std::process::exit(code);
}
