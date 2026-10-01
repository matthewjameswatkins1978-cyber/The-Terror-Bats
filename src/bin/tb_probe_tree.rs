//! Test probe: a three-level process tree (parent → child → grandchild).
//! Every level hangs while appending heartbeats to a shared file, so tests
//! can prove the whole tree lived and then died together. Not product surface.
use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut mode = "grandchild";
    let mut heartbeat: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--heartbeat" {
            heartbeat = args.get(i + 1).map(String::as_str);
            i += 2;
        } else {
            mode = args[i].as_str();
            i += 1;
        }
    }
    // Open the heartbeat file BEFORE spawning descendants, so the file's
    // existence never depends on winning a race with the supervisor.
    let mut heartbeat_file = heartbeat.map(|path| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap()
    });
    match mode {
        "parent" => spawn_next("child", heartbeat),
        "child" => spawn_next("grandchild", heartbeat),
        "grandchild" => {}
        other => {
            eprintln!("unknown tree mode `{other}`");
            std::process::exit(2);
        }
    }
    loop {
        if let Some(f) = heartbeat_file.as_mut() {
            let _ = writeln!(f, "alive");
            let _ = f.flush();
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

fn spawn_next(next: &str, heartbeat: Option<&str>) {
    let exe = std::env::current_exe().unwrap();
    let mut cmd = std::process::Command::new(exe);
    cmd.arg(next);
    if let Some(path) = heartbeat {
        cmd.arg("--heartbeat").arg(path);
    }
    // Deliberately not waited on: the descendant must outlive this call and
    // hang alongside us until the supervisor kills the whole tree (which also
    // reaps it; nothing here can zombie beyond the supervised run).
    #[allow(clippy::zombie_processes)]
    cmd.spawn().unwrap();
}
