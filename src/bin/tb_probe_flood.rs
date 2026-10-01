//! Test probe: writes deterministic volumes to stdout/stderr, interleaved.
//! Not product surface; exercised by `tests/supervisor.rs`.
use std::io::Write;

const CHUNK: usize = 65536;

fn main() {
    let mut stdout_mib: u64 = 0;
    let mut stderr_mib: u64 = 0;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--stdout-mib" => stdout_mib = args.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            "--stderr-mib" => stderr_mib = args.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            other => {
                eprintln!("unknown flag `{other}`");
                std::process::exit(2);
            }
        }
    }
    let out_chunk = vec![b'o'; CHUNK];
    let err_chunk = vec![b'e'; CHUNK];
    let out_chunks = stdout_mib * 1024 * 1024 / CHUNK as u64;
    let err_chunks = stderr_mib * 1024 * 1024 / CHUNK as u64;
    let mut out = std::io::stdout().lock();
    let mut err = std::io::stderr().lock();
    for i in 0..out_chunks.max(err_chunks) {
        if i < out_chunks {
            out.write_all(&out_chunk).unwrap();
        }
        if i < err_chunks {
            err.write_all(&err_chunk).unwrap();
        }
    }
}
