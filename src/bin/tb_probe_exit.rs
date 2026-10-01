//! Test probe: exits with a code and optionally reports environment state.
//! Not product surface; exercised by `tests/supervisor.rs`.
use std::io::{Read, Write};

fn main() {
    let mut code = 0;
    let mut print_env: Option<String> = None;
    let mut print_cwd = false;
    let mut echo_stdin = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--print-env" => print_env = args.next(),
            "--print-cwd" => print_cwd = true,
            "--echo-stdin" => echo_stdin = true,
            other => code = other.parse().unwrap_or(0),
        }
    }
    if let Some(name) = print_env {
        match std::env::var(&name) {
            Ok(value) => println!("{value}"),
            Err(_) => println!("<unset>"),
        }
    }
    if print_cwd {
        println!("{}", std::env::current_dir().unwrap().display());
    }
    if echo_stdin {
        let mut buf = Vec::new();
        std::io::stdin().read_to_end(&mut buf).unwrap();
        std::io::stdout().write_all(&buf).unwrap();
    }
    std::process::exit(code);
}
