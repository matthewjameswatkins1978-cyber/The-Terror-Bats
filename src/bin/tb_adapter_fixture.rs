//! Deterministic executable fixture for the Terror Bat M8 protocol tests.
use serde_json::{Value, json};
use std::io::{self, BufRead, Write};

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let mode = args.first().map(String::as_str).unwrap_or_default();
    let state_path = args.get(1).map(String::as_str);
    let mut input = String::new();
    io::stdin()
        .lock()
        .read_line(&mut input)
        .expect("request line");
    let request: Value = serde_json::from_str(&input).expect("request JSON");
    match request["kind"].as_str().unwrap_or("") {
        "describe" => describe(mode, state_path),
        "execute" => execute(mode, &request),
        _ => std::process::exit(2),
    }
}

fn describe(mode: &str, state_path: Option<&str>) {
    if mode == "describe-diagnostic" {
        eprintln!("fixture describe diagnostic");
    }
    if mode == "describe-hang" {
        std::thread::sleep(std::time::Duration::from_secs(10));
    }
    if mode == "describe-crash" {
        std::process::exit(9);
    }
    let name = match mode {
        "name-mismatch" => "different-name",
        "alpha" | "alpha-v2" | "alpha-drift" => "alpha",
        "beta" => "beta",
        _ => "fixture",
    };
    let protocol = if mode == "version-mismatch" {
        "terrorbat-adapter/v99"
    } else {
        "terrorbat-adapter/v1"
    };
    let version = match mode {
        "description-change" | "alpha-v2" => "2.0.0",
        "alpha-drift" => {
            let path = state_path.expect("alpha-drift state path");
            let count = std::fs::read_to_string(path)
                .ok()
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or_default();
            std::fs::write(path, (count + 1).to_string()).expect("update describe counter");
            if count == 0 { "1.0.0" } else { "2.0.0" }
        }
        _ => "1.0.0",
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
    if mode == "stdout-overflow" {
        io::stdout()
            .lock()
            .write_all(&vec![b'x'; 1_048_576 + 37])
            .expect("overflow stdout");
        return;
    }
    if mode == "stderr-overflow" {
        io::stderr()
            .lock()
            .write_all(&vec![b'y'; 1_048_576 + 37])
            .expect("overflow stderr");
    }
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
    if mode == "valid-stdout-overflow" {
        let mut bytes = serde_json::to_vec(&result).expect("serialize response");
        bytes.pop().expect("JSON object ends with a brace");
        let capture_limit = 1_048_576usize;
        assert!(bytes.len() + 2 < capture_limit);
        let padding = capture_limit - bytes.len() - 2;
        bytes.extend(std::iter::repeat_n(b' ', padding));
        bytes.extend_from_slice(b"}\n");
        bytes.extend_from_slice(&[b'x'; 37]);
        io::stdout()
            .lock()
            .write_all(&bytes)
            .expect("write valid overflow");
        return;
    }
    println!("{result}");
}
