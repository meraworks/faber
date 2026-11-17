//! Build the example through a real stdio MCP session.

use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

pub struct Client {
    child: Child,
    input: Option<ChildStdin>,
    output: mpsc::Receiver<String>,
    transcript: std::fs::File,
    id: u64,
}

impl Client {
    pub fn start(executable: PathBuf, transcript: PathBuf) -> Self {
        let mut child = Command::new(executable)
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("Start Faber");
        let input = child.stdin.take();
        let output = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                match line {
                    Ok(line) => {
                        if tx.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            child,
            input,
            output: rx,
            transcript: std::fs::File::create(transcript).unwrap(),
            id: 0,
        }
    }
    pub fn send(&mut self, value: Value) {
        writeln!(
            self.transcript,
            "{}",
            json!({"direction":"request","message":value})
        )
        .unwrap();
        let input = self.input.as_mut().unwrap();
        writeln!(input, "{value}").unwrap();
        input.flush().unwrap();
    }
    pub fn request(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        self.send(json!({"jsonrpc":"2.0","id":self.id,"method":method,"params":params}));
        loop {
            let line = self
                .output
                .recv_timeout(Duration::from_secs(60))
                .expect("MCP reply within 60 seconds");
            let value: Value = serde_json::from_str(&line).expect("MCP reply");
            writeln!(
                self.transcript,
                "{}",
                json!({"direction":"response","message":value})
            )
            .unwrap();
            if value.get("id").is_none() {
                continue;
            }
            assert_eq!(value["id"], self.id);
            assert!(value.get("error").is_none(), "{value}");
            return value["result"].clone();
        }
    }
    pub fn tool(&mut self, name: &str, arguments: Value) -> Value {
        let start = std::time::Instant::now();
        let result = self.request("tools/call", json!({"name":name,"arguments":arguments}));
        assert_ne!(result["isError"], true, "{name}: {result}");
        println!("{name}: {:.1} ms", start.elapsed().as_secs_f64() * 1000.);
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.input.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
