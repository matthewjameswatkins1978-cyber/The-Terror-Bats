use std::io::{self, BufRead, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("server") => {
            println!("READY");
            io::stdout().flush().unwrap();
            for line in io::stdin().lock().lines() {
                let line = line.unwrap();
                if line == "quit" {
                    break;
                }
                println!("ECHO:{line}");
                io::stdout().flush().unwrap();
            }
        }
        Some("state") => {
            let path = args.next().expect("state path");
            let old = std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let next = old + 1;
            std::fs::write(&path, next.to_string()).unwrap();
            println!("GEN:{next}");
            io::stdout().flush().unwrap();
            loop {
                thread::sleep(Duration::from_secs(60));
            }
        }
        Some("silent") => loop {
            thread::sleep(Duration::from_secs(60));
        },
        Some("flood") => {
            let bytes = vec![b'x'; 256 * 1024];
            io::stdout().write_all(&bytes).unwrap();
            io::stdout().flush().unwrap();
            loop {
                thread::sleep(Duration::from_secs(60));
            }
        }
        Some("stderr") => {
            eprintln!("READY-ERR");
            io::stderr().flush().unwrap();
            loop {
                thread::sleep(Duration::from_secs(60));
            }
        }
        Some("secret-echo") => {
            // Adversarial disclosure probe (test-only): prints the exact value
            // of the named variable to BOTH stdout and stderr, then exits 0.
            // This deliberately places secret material into captured output so
            // the disclosure-boundary regression can prove that capture is
            // verbatim and NOT sanitised. Never point this at a real secret
            // outside the regression's artificial sentinel.
            let name = args.next().expect("secret-echo variable name");
            let value = std::env::var(&name).unwrap_or_default();
            println!("SECRET_ECHO:{value}");
            eprintln!("SECRET_ECHO:{value}");
            io::stdout().flush().unwrap();
            io::stderr().flush().unwrap();
        }
        Some("secret-check") => {
            // Prints only whether the named variable is present; the value
            // itself never touches this fixture's output. That keeps THIS
            // fixture's capture clean, but it is not a general guarantee:
            // any target that prints a secret puts it into captured output
            // verbatim (see the secret-echo mode and the disclosure
            // regression).
            let name = args.next().expect("secret-check variable name");
            match std::env::var(&name) {
                Ok(v) if !v.is_empty() => println!("SECRET_OK"),
                _ => println!("SECRET_MISSING"),
            }
            io::stdout().flush().unwrap();
            loop {
                thread::sleep(Duration::from_secs(60));
            }
        }
        Some("tcp") => {
            let port = args.next().expect("port").parse::<u16>().unwrap();
            let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
            println!("READY");
            io::stdout().flush().unwrap();
            for stream in listener.incoming() {
                let _ = stream;
            }
        }
        Some("child") => {
            let exe = std::env::current_exe().unwrap();
            // The process supervisor under test owns and reaps this child group.
            #[allow(clippy::zombie_processes)]
            let child = Command::new(exe)
                .arg("idle-child")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            println!("CHILD_PID:{}", child.id());
            io::stdout().flush().unwrap();
            loop {
                thread::sleep(Duration::from_secs(60));
            }
        }
        Some("idle-child") => loop {
            thread::sleep(Duration::from_secs(60));
        },
        _ => std::process::exit(2),
    }
}
