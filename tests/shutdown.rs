//! The real binary, stopped with a signal. Unix only, like the signal.
#![cfg(unix)]

use std::io::Write as _;
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

struct Server {
    child: Child,
    address: String,
}

impl Server {
    fn start() -> Self {
        // Ask the system for a free port, then hand it to the server.
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let address = format!("127.0.0.1:{port}");
        let child = Command::new(env!("CARGO_BIN_EXE_rust-api-service"))
            .env_clear()
            .env("TOKEN_SECRET", "shutdown-test-secret-0123456789abcdef")
            .env("DATABASE_URL", "sqlite::memory:")
            .env("BIND_ADDR", &address)
            .env("SHUTDOWN_TIMEOUT_SECS", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let server = Self { child, address };
        let started = Instant::now();
        while TcpStream::connect(&server.address).is_err() {
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "the server never listened"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        // Listening comes just before the signal handlers are installed.
        std::thread::sleep(Duration::from_millis(200));
        server
    }

    fn terminate(&self) {
        let sent = Command::new("kill")
            .args(["-TERM", &self.child.id().to_string()])
            .status()
            .unwrap();
        assert!(sent.success());
    }

    fn wait(&mut self, limit: Duration) -> Option<ExitStatus> {
        let started = Instant::now();
        while started.elapsed() < limit {
            if let Some(status) = self.child.try_wait().unwrap() {
                return Some(status);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn an_idle_server_exits_cleanly_on_sigterm() {
    let mut server = Server::start();
    assert!(
        server.wait(Duration::from_secs(2)).is_none(),
        "the shutdown limit must not start counting before a signal"
    );
    server.terminate();
    let status = server.wait(Duration::from_secs(5)).expect("still running");
    assert_eq!(status.code(), Some(0));
}

#[test]
fn a_request_that_never_finishes_cannot_hold_up_shutdown() {
    let mut server = Server::start();
    let mut stuck = TcpStream::connect(&server.address).unwrap();
    stuck
        .write_all(
            b"POST /auth/register HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\n\
              Content-Length: 100\r\n\r\n{",
        )
        .unwrap();
    std::thread::sleep(Duration::from_millis(200));
    server.terminate();
    let status = server
        .wait(Duration::from_secs(6))
        .expect("still running five seconds after the one-second limit");
    assert_eq!(
        status.code(),
        Some(1),
        "giving up on a request is not a clean exit"
    );
}
