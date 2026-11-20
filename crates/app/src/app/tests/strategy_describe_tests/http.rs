use super::*;
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
};

pub(super) struct Provider {
    pub endpoint: String,
    pub requests: Arc<Mutex<Vec<Value>>>,
    address: std::net::SocketAddr,
    handle: Option<std::thread::JoinHandle<()>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
}
impl Drop for Provider {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        let _ = std::net::TcpStream::connect(self.address);
        if let Some(handle) = self.handle.take() {
            let result = handle.join();
            if !std::thread::panicking() {
                result.unwrap();
            }
        }
    }
}
pub(super) fn provider(replies: Vec<Value>) -> Provider {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = format!(
        "http://{}/v1/chat/completions",
        listener.local_addr().unwrap()
    );
    let requests = Arc::new(Mutex::new(vec![]));
    let captured = Arc::clone(&requests);
    let address = listener.local_addr().unwrap();
    let stop = Arc::<std::sync::atomic::AtomicBool>::default();
    let stopped = stop.clone();
    let handle = std::thread::spawn(move || {
        let mut replies = replies.into_iter();
        loop {
            let (mut stream, _) = listener.accept().unwrap();
            if stopped.load(std::sync::atomic::Ordering::Acquire) {
                break;
            }
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                .unwrap();
            let mut reader = BufReader::new(&stream);
            let mut headers = String::new();
            loop {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).unwrap() > 0);
                if line == "\r\n" {
                    break;
                }
                headers.push_str(&line);
            }
            assert!(headers.starts_with("POST /v1/chat/completions "));
            assert!(
                headers
                    .to_lowercase()
                    .contains("authorization: bearer fixture-credential")
            );
            let len = headers
                .to_lowercase()
                .lines()
                .find_map(|line| {
                    line.strip_prefix("content-length: ")
                        .and_then(|n| n.parse::<usize>().ok())
                })
                .unwrap();
            let mut body = vec![0; len];
            reader.read_exact(&mut body).unwrap();
            captured
                .lock()
                .unwrap()
                .push(serde_json::from_slice(&body).unwrap());
            let reply = replies.next().expect("unexpected additional HTTP request");
            let payload=json!({"choices":[{"finish_reason":"stop","message":{"content":reply.to_string()}}]}).to_string();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{payload}",payload.len()).unwrap();
        }
    });
    Provider {
        endpoint,
        requests,
        stop,
        address,
        handle: Some(handle),
    }
}
