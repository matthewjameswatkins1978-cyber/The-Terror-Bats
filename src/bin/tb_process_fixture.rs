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
        Some("pipe-heir") => {
            // Supervised parent: spawns an heir that INHERITS our stdout and
            // stderr (the supervisor's capture pipes), announces, and exits
            // 0 immediately. The heir outlives the parent inside the owned
            // process group / Job. Args: port die_after_secs.
            let port: u16 = args.next().expect("port").parse().unwrap();
            let die_after: u64 = args.next().expect("die_after").parse().unwrap();
            let exe = std::env::current_exe().unwrap();
            // The process supervisor under test owns and reaps this child group.
            #[allow(clippy::zombie_processes)]
            Command::new(exe)
                .arg("pipe-heir-child")
                .arg(port.to_string())
                .arg(die_after.to_string())
                .stdin(Stdio::null())
                .spawn()
                .unwrap();
            println!("PARENT_EXITING");
            io::stdout().flush().unwrap();
        }
        Some("pipe-heir-child") => {
            // Inherits the supervisor's capture pipes: keeps them open after
            // the parent exits. Binds a liveness port, announces over the
            // inherited pipe, then self-destructs after die_after_secs — the
            // external watchdog bounding every test even if supervision hangs.
            heir_child(&mut args);
        }
        Some("null-heir") => {
            // Like pipe-heir, but the heir's stdio is nulled: pumps see EOF
            // at parent exit while the heir still lives in the owned group.
            let port: u16 = args.next().expect("port").parse().unwrap();
            let die_after: u64 = args.next().expect("die_after").parse().unwrap();
            let exe = std::env::current_exe().unwrap();
            // The process supervisor under test owns and reaps this child group.
            #[allow(clippy::zombie_processes)]
            Command::new(exe)
                .arg("null-heir-child")
                .arg(port.to_string())
                .arg(die_after.to_string())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            println!("PARENT_EXITING");
            io::stdout().flush().unwrap();
        }
        Some("null-heir-child") => {
            // Null stdio: nothing observable through capture. The liveness
            // port is the only OS-level proof of life. Self-destruct bounds.
            heir_child(&mut args);
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

fn heir_child(args: &mut std::iter::Skip<std::env::Args>) {
    // Shared heir body: bind a 127.0.0.1 liveness listener (a successful
    // connect proves OS-level aliveness independent of any receipt claim),
    // announce over stdout when it is inherited, then exit on our own after
    // die_after_secs so no test can strand a survivor past its watchdog.
    let port: u16 = args.next().expect("heir port").parse().unwrap();
    let die_after: u64 = args.next().expect("heir die_after").parse().unwrap();
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("heir liveness port");
    println!("HEIR_PORT:{port}");
    io::stdout().flush().unwrap();
    let _ = listener;
    thread::sleep(Duration::from_secs(die_after));
}
