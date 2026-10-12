//! Test probe: a three-level process tree (parent → child → grandchild).
//! Every level hangs while appending heartbeats, so tests can prove the whole
//! tree lived and then died together. Not product surface.
use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut mode = "grandchild";
    let mut heartbeat: Option<&str> = None;
    let mut marker_dir: Option<&str> = None;
    let mut skip_child = false;
    let mut skip_grandchild = false;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--heartbeat" {
            heartbeat = args.get(i + 1).map(String::as_str);
            i += 2;
        } else if args[i] == "--marker-dir" {
            marker_dir = args.get(i + 1).map(String::as_str);
            i += 2;
        } else if args[i] == "--skip-child" {
            skip_child = true;
            i += 1;
        } else if args[i] == "--skip-grandchild" {
            skip_grandchild = true;
            i += 1;
        } else {
            mode = args[i].as_str();
            i += 1;
        }
    }

    // Role-tagged marker file written immediately upon process entry.
    if let Some(dir) = marker_dir {
        let path = std::path::Path::new(dir).join(format!("{mode}.marker"));
        let _ = std::fs::write(path, format!("{mode}:ready\n"));
    }

    // Role-tagged initial heartbeat entry.
    let mut heartbeat_file = heartbeat.map(|path| {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        let _ = writeln!(file, "{mode}:alive");
        let _ = file.flush();
        file
    });

    match mode {
        "parent" => {
            if !skip_child {
                spawn_next("child", heartbeat, marker_dir, skip_child, skip_grandchild);
            }
        }
        "child" => {
            if !skip_grandchild {
                spawn_next(
                    "grandchild",
                    heartbeat,
                    marker_dir,
                    skip_child,
                    skip_grandchild,
                );
            }
        }
        "grandchild" => {}
        other => {
            eprintln!("unknown tree mode `{other}`");
            std::process::exit(2);
        }
    }

    loop {
        if let Some(f) = heartbeat_file.as_mut() {
            let _ = writeln!(f, "{mode}:alive");
            let _ = f.flush();
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

fn spawn_next(
    next: &str,
    heartbeat: Option<&str>,
    marker_dir: Option<&str>,
    skip_child: bool,
    skip_grandchild: bool,
) {
    let exe = std::env::current_exe().unwrap();
    let mut cmd = std::process::Command::new(exe);
    cmd.arg(next);
    if let Some(path) = heartbeat {
        cmd.arg("--heartbeat").arg(path);
    }
    if let Some(dir) = marker_dir {
        cmd.arg("--marker-dir").arg(dir);
    }
    if skip_child {
        cmd.arg("--skip-child");
    }
    if skip_grandchild {
        cmd.arg("--skip-grandchild");
    }
    // Deliberately not waited on: the descendant must outlive this call and
    // hang alongside us until the supervisor kills the whole tree (which also
    // reaps it; nothing here can zombie beyond the supervised run).
    #[allow(clippy::zombie_processes)]
    cmd.spawn().unwrap();
}
