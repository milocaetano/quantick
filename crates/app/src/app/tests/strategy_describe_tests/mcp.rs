//! Drive the actual stdio executable, pumping the same desktop frames as a human.
use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{OnceLock, mpsc as channel},
};

fn executable() -> &'static Path {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT.get_or_init(|| {
        let test = std::env::current_exe().unwrap();
        let profile = test.parent().unwrap().parent().unwrap();
        let profile_name = match profile.file_name().unwrap().to_str().unwrap() {
            "debug" => "test",
            name => name,
        };
        // Selecting both packages unifies shared dependency features with the
        // app build. Only the real MCP binary is requested from Cargo.
        let output = Command::new(env!("CARGO"))
            .args([
                "build",
                "--locked",
                "-p",
                "quantick-app",
                "-p",
                "quantick-mcp",
                "--bin",
                "quantick-mcp",
                "--message-format=json-render-diagnostics",
                "--profile",
                profile_name,
                "--target-dir",
            ])
            .arg(profile.parent().unwrap())
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .stderr(Stdio::inherit())
            .output()
            .expect("run Cargo to resolve the MCP fixture");
        assert!(
            output.status.success(),
            "building the MCP fixture failed: {}",
            output.status
        );
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
            .with_file_name("mcp")
            .join("Cargo.toml");
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("Cargo JSON message"))
            .filter(|msg| {
                msg["reason"] == "compiler-artifact"
                    && msg["target"]["name"] == "quantick-mcp"
                    && msg["manifest_path"].as_str().map(Path::new) == Some(&manifest)
            })
            .find_map(|msg| msg["executable"].as_str().map(PathBuf::from))
            .expect("Cargo reported no executable for this checkout's MCP binary")
    })
}

pub(super) struct Client {
    process: Child,
    input: ChildStdin,
    replies: channel::Receiver<Value>,
    next: u64,
    instance: Value,
}
impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}
impl Client {
    pub(super) fn start(
        app: &mut QuantickApp,
        ctx: &egui::Context,
        directory: &Path,
        profile: &str,
    ) -> Self {
        let mut process = Command::new(executable())
            .args(["serve", "--profile", profile, "--instances-dir"])
            .arg(directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let input = process.stdin.take().unwrap();
        let output = process.stdout.take().unwrap();
        let (send, replies) = channel::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines().map_while(Result::ok) {
                if send.send(serde_json::from_str(&line).unwrap()).is_err() {
                    break;
                }
            }
        });
        let mut client = Self {
            process,
            input,
            replies,
            next: 0,
            instance: Value::Null,
        };
        let reply=client.rpc(app,ctx,"initialize",json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"describe-test","version":"1"}}));
        assert_eq!(reply["result"]["protocolVersion"], "2025-06-18");
        writeln!(
            client.input,
            "{}",
            json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        )
        .unwrap();
        let summary = client.tool(app, ctx, "quantick_describe", json!({}));
        client.instance = summary["instances"][0]["instance_id"].clone();
        assert!(client.instance.is_string(), "{summary}");
        client
    }
    fn rpc(
        &mut self,
        app: &mut QuantickApp,
        ctx: &egui::Context,
        method: &str,
        params: Value,
    ) -> Value {
        self.next += 1;
        writeln!(
            self.input,
            "{}",
            json!({"jsonrpc":"2.0","id":self.next,"method":method,"params":params})
        )
        .unwrap();
        self.input.flush().unwrap();
        let deadline = std::time::Instant::now() + GATEWAY_TEST_WAIT;
        loop {
            run_frame(app, ctx);
            if let Ok(value) = self
                .replies
                .recv_timeout(std::time::Duration::from_millis(5))
            {
                assert_eq!(value["id"], self.next);
                assert!(value.get("error").is_none(), "{value}");
                return value;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "MCP response timed out"
            );
        }
    }
    fn tool(
        &mut self,
        app: &mut QuantickApp,
        ctx: &egui::Context,
        name: &str,
        mut arguments: Value,
    ) -> Value {
        if !self.instance.is_null() {
            arguments["instance_id"] = self.instance.clone();
        }
        let reply = self.rpc(
            app,
            ctx,
            "tools/call",
            json!({"name":name,"arguments":arguments}),
        );
        reply["result"]["structuredContent"].clone()
    }
    pub(super) fn availability(
        &mut self,
        app: &mut QuantickApp,
        ctx: &egui::Context,
        enabled: bool,
    ) {
        let described = self.tool(app, ctx, "quantick_describe", json!({}));
        let found = self.tool(
            app,
            ctx,
            "quantick_search_capabilities",
            json!({"query":"strategy.describe."}),
        );
        let rows = found["capabilities"].as_array().expect("search result");
        assert_eq!(rows.len(), 5);
        for row in rows {
            let full = described["result"]["capabilities"]
                .as_array()
                .unwrap()
                .iter()
                .find(|item| item["id"] == row["id"])
                .unwrap();
            assert_eq!(row["availability"], full["availability"]);
            let available = enabled || row["id"] == "strategy.describe.read";
            assert_eq!(
                row["availability"]["status"],
                if available {
                    "available"
                } else {
                    "unavailable"
                }
            );
            if !available {
                assert_eq!(
                    row["availability"]["reason_code"],
                    "strategy.describe_unavailable"
                );
            }
        }
    }
    pub(super) fn call(
        &mut self,
        app: &mut QuantickApp,
        ctx: &egui::Context,
        op: &str,
        payload: Value,
        key: &str,
    ) -> Value {
        let mut args = json!({"capability_id":format!("strategy.describe.{op}"),"capability_version":1,"payload":payload});
        if op != "read" {
            args["idempotency_key"] = key.into();
        }
        self.tool(app, ctx, "quantick_invoke", args)
    }
}
