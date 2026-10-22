#![cfg(unix)]

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn sigterm_stops_the_hub_cleanly() {
    let data = tempfile::tempdir().unwrap();
    let mut hub = Command::new(env!("CARGO_BIN_EXE_kelpie-hub"))
        .args(["--dev", "--listen", "127.0.0.1:0", "--data"])
        .arg(data.path())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stderr = BufReader::new(hub.stderr.take().unwrap());
    let mut line = String::new();
    stderr.read_line(&mut line).unwrap();
    assert!(line.contains("listening"), "{line}");

    let killed = Command::new("kill")
        .args(["-TERM", &hub.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());

    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = hub.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "the hub ignored SIGTERM");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(status.success(), "{status:?}");
}
