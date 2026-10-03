//! Deterministic executable fixture for the Terror Bat M8 protocol tests.
use serde_json::{Value, json};
use std::io::{self, BufRead};

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    let mut input = String::new();
    io::stdin()
        .lock()
        .read_line(&mut input)
        .expect("request line");
    let request: Value = serde_json::from_str(&input).expect("request JSON");
    match request["kind"].as_str().unwrap_or("") {
        "describe" => describe(&mode),
        "execute" => execute(&mode, &request),
        _ => std::process::exit(2),
    }
}

fn describe(mode: &str) {
    if mode == "describe-diagnostic" {
        eprintln!("fixture describe diagnostic");
    }
    if mode == "describe-hang" {
        std::thread::sleep(std::time::Duration::from_secs(10));
    }
    if mode == "describe-crash" {
        std::process::exit(9);
    }
    let name = if mode == "name-mismatch" {
        "different-name"
    } else {
        "fixture"
    };
    let protocol = if mode == "version-mismatch" {
        "terrorbat-adapter/v99"
    } else {
        "terrorbat-adapter/v1"
    };
    let version = if mode == "description-change" {
        "2.0.0"
    } else {
        "1.0.0"
    };
    let value = json!({
        "protocol": protocol,
        "kind": "description",
        "name": name,
        "version": version,
        "actions": {
            "mutate": { "requires": ["fs.write"] },
            "observe": { "requires": [] }
        }
    });
    println!("{value}");
}

fn execute(mode_arg: &str, request: &Value) {
    if mode_arg == "execute-crash" {
        std::process::exit(9);
    }
    let payload = &request["payload"];
    let mode = payload["mode"].as_str().unwrap_or(mode_arg);
    if mode == "hang" {
        std::thread::sleep(std::time::Duration::from_secs(10));
    }
    if mode == "diagnostic" {
        eprintln!("fixture protocol diagnostic");
    }
    if mode == "malformed" {
        println!("not-json");
        return;
    }
    if mode == "multiple" {
        println!("{{}}\nnoise");
        return;
    }
    if mode == "extra-output" {
        println!("startup chatter");
    }
    let version = if mode == "version-mismatch" {
        "terrorbat-adapter/v99"
    } else {
        "terrorbat-adapter/v1"
    };
    let stdout = match mode {
        "logical" => "logical stdout",
        "nonzero" => "logical command failed",
        _ => "",
    };
    let stderr = if mode == "logical" {
        "logical stderr"
    } else {
        ""
    };
    let exit_code = if mode == "nonzero" { 17 } else { 0 };
    if mode == "mutate" {
        let root = request["context"]["worktree"]
            .as_str()
            .expect("worktree context");
        std::fs::write(
            std::path::Path::new(root).join("m8-adapter-output.txt"),
            "written by external adapter\n",
        )
        .expect("fixture worktree write");
    }
    let status = match mode {
        "invalid" => "invalid",
        "policy-denied" => "policy_denied",
        "infra" => "infrastructure_error",
        _ => "completed",
    };
    let result = json!({
        "protocol": version,
        "kind": "result",
        "request_id": request["request_id"],
        "status": status,
        "exit_code": exit_code,
        "stdout": stdout,
        "stderr": stderr
    });
    println!("{result}");
}
