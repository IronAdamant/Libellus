#![allow(dead_code)]
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const SERVER: &str = env!("CARGO_BIN_EXE_libellus-mcp");
pub const CLI: &str = env!("CARGO_BIN_EXE_libellus");

pub fn project(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("libellus-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

pub fn init(dir: &Path) {
    let out = Command::new(CLI)
        .arg("init")
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Send raw bytes to the MCP server (one or more lines), close stdin, and
/// return each reply line parsed loosely as text.
pub fn talk(dir: &Path, input: &[u8]) -> Vec<String> {
    talk_env(dir, input, &[])
}

/// `talk`, with extra environment variables on the server process.
pub fn talk_env(dir: &Path, input: &[u8], env: &[(&str, &str)]) -> Vec<String> {
    let mut cmd = Command::new(SERVER);
    cmd.current_dir(dir)
        .env_remove("LIBELLUS_DIR")
        .env_remove("LIBELLUS_TOOLS")
        .env_remove("LIBELLUS_DISABLE")
        .env_remove("LIBELLUS_AGENT")
        .env_remove("SWARMNOTES_DIR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for (key, value) in env {
        cmd.env(key, value);
    }
    let mut child = cmd.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let data = input.to_vec();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&data);
    });
    let out = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(out.status.success(), "server exited with {:?}", out.status);
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(String::from)
        .collect()
}

pub fn call(name: &str, args: &str, id: u32) -> String {
    format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"tools/call\",\"params\":{{\"name\":\"{name}\",\"arguments\":{args}}}}}\n")
}

pub fn is_error(reply: &str) -> bool {
    reply.contains("\"isError\":true")
}

pub fn is_ok(reply: &str) -> bool {
    reply.contains("\"isError\":false")
}

pub fn strays(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(d: &Path, out: &mut Vec<String>) {
        if let Ok(rd) = std::fs::read_dir(d) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.file_name().unwrap().to_string_lossy().contains(".tmp-") {
                    out.push(p.display().to_string());
                }
            }
        }
    }
    walk(dir, &mut out);
    out
}
