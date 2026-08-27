//! The binary over real pipes: standard output carries MCP frames only, from
//! startup through errors to shutdown, and with no instance running the
//! adapter says so instead of starting one.

use std::{
    io::{Read as _, Write as _},
    process::{Command, Stdio},
};

use serde_json::{Value, json};

fn scratch_directory(name: &str) -> std::path::PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "quantick-mcp-stdio-{name}-{}-{}",
        std::process::id(),
        line!()
    ));
    let _ = std::fs::remove_dir_all(&directory);
    directory
}

fn frames(stdout: &[u8]) -> Vec<Value> {
    let text = String::from_utf8(stdout.to_vec()).expect("stdout is UTF-8");
    text.lines()
        .map(|line| {
            let frame: Value = serde_json::from_str(line)
                .unwrap_or_else(|error| panic!("stdout line is not JSON ({error}): {line:?}"));
            assert_eq!(
                frame["jsonrpc"], "2.0",
                "stdout line is not a JSON-RPC frame: {line}"
            );
            frame
        })
        .collect()
}

#[test]
fn startup_errors_and_shutdown_emit_only_mcp_frames_on_stdout() {
    let directory = scratch_directory("smoke");
    let mut child = Command::new(env!("CARGO_BIN_EXE_quantick-mcp"))
        .arg("--instances-dir")
        .arg(&directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the adapter binary starts");
    {
        let mut stdin = child.stdin.take().expect("piped stdin");
        let script = [
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}).to_string(),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}).to_string(),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}).to_string(),
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"quantick_describe","arguments":{}}}).to_string(),
            "this is not a frame".to_owned(),
            json!({"jsonrpc":"2.0","id":4,"method":"ping"}).to_string(),
        ]
        .join("\n");
        stdin.write_all(script.as_bytes()).unwrap();
        stdin.write_all(b"\n").unwrap();
        // Closing stdin is how the client shuts the server down.
    }
    let output = child.wait_with_output().expect("the adapter exits");
    assert!(output.status.success(), "exit status: {:?}", output.status);

    let frames = frames(&output.stdout);
    assert_eq!(frames.len(), 5, "one frame per answered line: {frames:#?}");
    assert_eq!(frames[0]["id"], 1);
    assert_eq!(frames[0]["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(frames[1]["id"], 2);
    assert_eq!(frames[1]["result"]["tools"].as_array().unwrap().len(), 10);
    assert_eq!(frames[2]["id"], 3);
    let describe = &frames[2]["result"];
    assert_eq!(describe["isError"], false);
    assert!(
        describe["structuredContent"]["instances"]
            .as_array()
            .unwrap()
            .is_empty(),
        "no instance is running and none was started"
    );
    assert!(
        !describe["structuredContent"]["next_steps"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(frames[3]["id"].is_null());
    assert_eq!(frames[3]["error"]["code"], -32700);
    assert_eq!(frames[4]["id"], 4);
    assert_eq!(frames[4]["result"], json!({}));
    assert!(
        !directory.exists(),
        "discovery must not create the instances directory"
    );
    // Diagnostics are allowed on stderr and nowhere else.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("never starts Quantick"));
}

#[test]
fn an_unavailable_profile_is_refused_before_serving() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_quantick-mcp"))
        .args(["--profile", "developer"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        output.stdout.is_empty(),
        "nothing reaches stdout on a refused start"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not available"));
}

#[test]
fn setup_prints_the_registration_command_with_the_binary_path_and_no_secret() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_quantick-mcp"))
        .args(["setup", "--client", "claude"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut stdout)
        .unwrap();
    assert!(child.wait().unwrap().success());
    assert!(stdout.contains("claude mcp add --transport stdio --scope local quantick -- "));
    assert!(stdout.contains("quantick-mcp"));
    assert!(stdout.contains("--profile observer"));
    assert!(!stdout.contains("bearer"));
}
