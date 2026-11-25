//! Release-mode timings for the hot paths. Not part of the product.
//!
//! Build the binaries, then write a markdown report:
//!
//! ```text
//! cargo build --release --locked --bins --example perf
//! cargo run --release --locked --example perf -- docs/perf/baseline-1.0.0.md
//! ```
//!
//! Figures are wall time from `Instant`. Median is the upper middle of the
//! sorted samples. p95 is the nearest rank, `ceil(0.95 * n) - 1`.

use libellus::fsx;
use libellus::json::Value;
use libellus::ledger::{self, Entry};
use libellus::store::Store;
use libellus::tool::Tool;
use libellus::tools::TOOLS;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let out = std::env::args()
        .nth(1)
        .ok_or_else(|| "usage: perf OUT.md [title]".to_string())?;
    let title = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "baseline 1.0.0".to_string());
    let bin_dir = release_dir()?;
    let cli = bin_dir.join("libellus");
    let mcp = bin_dir.join("libellus-mcp");
    if !cli.is_file() || !mcp.is_file() {
        return Err(format!(
            "missing release binaries in {} (build --bins --release first)",
            bin_dir.display()
        ));
    }
    let root = std::env::temp_dir().join(format!("libellus-perf-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let store = root.join(".libellus");
    std::env::set_var("LIBELLUS_DIR", &store);
    Store::init()?;

    let mut md = String::new();
    md.push_str(&format!("# Libellus performance {title}\n\n"));
    md.push_str("Release build. Same harness for the baseline and later runs.\n\n");
    md.push_str(&machine());
    md.push_str(
        "\nMedian is the upper middle sample. p95 is the nearest rank, ceil(0.95 * n) - 1.\n",
    );
    md.push_str("Times are microseconds of wall time unless a row says otherwise.\n\n");

    let note = tool("libellus_note")?;
    let ls = tool("libellus_ls")?;
    let get = tool("libellus_get")?;
    let brief = tool("libellus_brief")?;
    let spend = tool("libellus_spend")?;
    let ledger = tool("libellus_ledger")?;

    let mut rows: Vec<Row> = Vec::new();
    let mut sizes: Vec<String> = Vec::new();
    let scales = [10usize, 1_000, 10_000];
    let mut have = 0usize;
    for scale in scales {
        eprint!("notes {have} -> {scale} ... ");
        let writes = time_notes(note, have, scale)?;
        eprintln!("{} writes", writes.len());
        rows.push(Row::from_us(
            &format!("note write (growing to {scale})"),
            writes,
        ));
        have = scale;
        sizes.push(store_size(&store, scale, false)?);
        let repeats = if scale >= 10_000 { 5 } else { 15 };
        rows.push(Row::from_us(
            &format!("ls limit 50 at {scale} notes"),
            time_tool(ls, &ls_args(50), repeats)?,
        ));
        rows.push(Row::from_us(
            &format!("get one note at {scale} notes"),
            time_tool(get, &get_args(scale / 2), repeats)?,
        ));
        rows.push(Row::from_us(
            &format!("brief at {scale} notes, no ledger yet"),
            time_tool(brief, &brief_args(), repeats)?,
        ));
    }

    eprint!("ledger grow to 1000 ... ");
    let early = time_spends(spend, 1_000)?;
    eprintln!("done");
    rows.push(Row::from_us(
        "spend append, entries 0 to 9",
        early.iter().take(10).copied().collect(),
    ));
    rows.push(Row::from_us(
        "spend append, entries 10 to 999",
        early.iter().skip(10).copied().collect(),
    ));
    seed_ledger(&store, 10_000)?;
    rows.push(Row::from_us(
        "spend append with 10000 entries already stored",
        time_spends(spend, 20)?,
    ));
    rows.push(Row::from_us(
        "ledger read at 10000 entries",
        time_tool(ledger, &ledger_args(), 10)?,
    ));
    rows.push(Row::from_us(
        "brief at 10000 notes and 10000 ledger entries",
        time_tool(brief, &brief_args(), 8)?,
    ));
    sizes.push(store_size(&store, 10_000, true)?);

    eprint!("cli and mcp ... ");
    rows.push(Row::from_us(
        "CLI cold start, libellus --version",
        time_cmd(&cli, &["--version"], &root, 30)?,
    ));
    rows.push(Row::from_us(
        "CLI cold brief at 10000 notes",
        time_cmd(&cli, &["brief", "--agent", "bench"], &root, 8)?,
    ));
    rows.push(Row::from_us(
        "MCP cold start plus one brief",
        time_mcp_cold(&mcp, &root, 8)?,
    ));
    rows.push(Row::from_us(
        "MCP warm brief round trip",
        time_mcp_warm(&mcp, &root, 30)?,
    ));
    eprintln!("done");

    eprint!("large reads ... ");
    let (fix_rows, fix_notes) = time_fixtures(&root, get)?;
    rows.extend(fix_rows);
    eprintln!("done");

    md.push_str("## Timings\n\n");
    md.push_str("| Path | Runs | Median us | p95 us |\n|---|---:|---:|---:|\n");
    for row in &rows {
        md.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            row.name, row.n, row.median, row.p95
        ));
    }
    md.push_str("\n## Store size\n\n");
    md.push_str(&sizes.concat());
    md.push_str("\n## Large fixtures\n\n");
    md.push_str(&fix_notes);
    md.push_str(
        "\nNote writes and spends go through the same tool handlers as MCP.\n\
         The 10000-entry spend sample starts from a ledger written in one go, then times real appends.\n\
         The smaller spend rows are the real appends that grew the file to 1000 entries.\n",
    );

    if let Some(parent) = Path::new(&out).parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(&out, &md).map_err(|e| format!("cannot write {out}: {e}"))?;
    println!("{md}");
    let _ = fs::remove_dir_all(&root);
    Ok(())
}

struct Row {
    name: String,
    n: usize,
    median: u128,
    p95: u128,
}

impl Row {
    fn from_us(name: &str, mut samples: Vec<u128>) -> Row {
        assert!(!samples.is_empty(), "{name}");
        samples.sort_unstable();
        let n = samples.len();
        let median = samples[n / 2];
        let rank = (n * 95 + 99) / 100;
        let p95 = samples[rank.saturating_sub(1)];
        Row {
            name: name.to_string(),
            n,
            median,
            p95,
        }
    }
}

fn tool(name: &str) -> Result<&'static Tool, String> {
    TOOLS
        .iter()
        .find(|t| t.name == name)
        .ok_or_else(|| format!("missing tool {name}"))
}

fn run_tool(tool: &Tool, args: &Value) -> Result<(), String> {
    let parsed = tool.validate(args)?;
    (tool.run)(&parsed).map(|_| ())
}

fn time_tool(tool: &Tool, args: &Value, n: usize) -> Result<Vec<u128>, String> {
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let start = Instant::now();
        run_tool(tool, args)?;
        out.push(start.elapsed().as_micros());
    }
    Ok(out)
}

fn time_notes(tool: &Tool, from: usize, to: usize) -> Result<Vec<u128>, String> {
    let mut out = Vec::with_capacity(to - from);
    for i in from..to {
        let args = Value::obj(vec![
            ("agent", Value::str("bench")),
            ("kind", Value::str("finding")),
            ("summary", Value::String(format!("note {i}"))),
            ("id", Value::String(format!("n{i}"))),
            ("body", Value::String(format!("detail {i}"))),
            (
                "evidence",
                Value::Array(vec![Value::str("chisel:src/net.rs")]),
            ),
        ]);
        let start = Instant::now();
        run_tool(tool, &args)?;
        out.push(start.elapsed().as_micros());
        if (i + 1) % 2000 == 0 {
            eprint!("{i} ");
        }
    }
    Ok(out)
}

fn time_spends(tool: &Tool, n: usize) -> Result<Vec<u128>, String> {
    let args = Value::obj(vec![
        ("agent", Value::str("bench")),
        ("task", Value::str("work")),
        ("step", Value::str("run")),
        ("tokens_in", Value::uint(10)),
        ("tokens_out", Value::uint(2)),
        ("cost_micro_usd", Value::uint(5)),
    ]);
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let start = Instant::now();
        run_tool(tool, &args)?;
        out.push(start.elapsed().as_micros());
    }
    Ok(out)
}

fn seed_ledger(store: &Path, n: usize) -> Result<(), String> {
    let path = store.join("agents/bench/ledger.jsonl");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let entry = Entry {
        ts: "2026-11-25T00:00:00Z".into(),
        agent: "bench".into(),
        task: "work".into(),
        step: Some("run".into()),
        model: None,
        tokens_in: 10,
        tokens_out: 2,
        cost_micro_usd: 5,
        note: None,
    };
    let line = entry.to_line();
    let mut f = File::create(&path).map_err(|e| e.to_string())?;
    for _ in 0..n {
        writeln!(f, "{line}").map_err(|e| e.to_string())?;
    }
    f.sync_all().map_err(|e| e.to_string())?;
    // The next real append must accept this file.
    let count = ledger::read(&Store::open()?, "bench")?.len();
    if count != n {
        return Err(format!("seeded ledger has {count} entries, wanted {n}"));
    }
    Ok(())
}

fn ls_args(limit: u64) -> Value {
    Value::obj(vec![
        ("agent", Value::str("bench")),
        ("limit", Value::uint(limit)),
    ])
}

fn get_args(i: usize) -> Value {
    Value::obj(vec![
        ("agent", Value::str("bench")),
        ("id", Value::String(format!("n{i}"))),
    ])
}

fn brief_args() -> Value {
    Value::obj(vec![("agent", Value::str("bench"))])
}

fn ledger_args() -> Value {
    Value::obj(vec![("agent", Value::str("bench"))])
}

fn time_cmd(bin: &Path, args: &[&str], cwd: &Path, n: usize) -> Result<Vec<u128>, String> {
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let start = Instant::now();
        let done = Command::new(bin)
            .args(args)
            .current_dir(cwd)
            .env("LIBELLUS_DIR", cwd.join(".libellus"))
            .output()
            .map_err(|e| e.to_string())?;
        let us = start.elapsed().as_micros();
        if !done.status.success() {
            return Err(format!(
                "{} {} failed: {}",
                bin.display(),
                args.join(" "),
                String::from_utf8_lossy(&done.stderr)
            ));
        }
        out.push(us);
    }
    Ok(out)
}

fn time_mcp_cold(bin: &Path, cwd: &Path, n: usize) -> Result<Vec<u128>, String> {
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let start = Instant::now();
        mcp_exchange(bin, cwd, 1)?;
        out.push(start.elapsed().as_micros());
    }
    Ok(out)
}

fn time_mcp_warm(bin: &Path, cwd: &Path, n: usize) -> Result<Vec<u128>, String> {
    let mut child = Command::new(bin)
        .current_dir(cwd)
        .env("LIBELLUS_DIR", cwd.join(".libellus"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-06-18"}}}}"#
    )
    .map_err(|e| e.to_string())?;
    stdin.flush().map_err(|e| e.to_string())?;
    let mut line = String::new();
    stdout.read_line(&mut line).map_err(|e| e.to_string())?;
    let call = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"libellus_brief","arguments":{"agent":"bench"}}}"#;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        line.clear();
        let start = Instant::now();
        writeln!(stdin, "{call}").map_err(|e| e.to_string())?;
        stdin.flush().map_err(|e| e.to_string())?;
        stdout.read_line(&mut line).map_err(|e| e.to_string())?;
        out.push(start.elapsed().as_micros());
    }
    drop(stdin);
    let _ = child.wait();
    Ok(out)
}

fn mcp_exchange(bin: &Path, cwd: &Path, calls: usize) -> Result<(), String> {
    let mut child = Command::new(bin)
        .current_dir(cwd)
        .env("LIBELLUS_DIR", cwd.join(".libellus"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut stdin = child.stdin.take().unwrap();
    let mut body = String::from(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-06-18\"}}\n",
    );
    for i in 0..calls {
        body.push_str(&format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{},\"method\":\"tools/call\",\"params\":{{\"name\":\"libellus_brief\",\"arguments\":{{\"agent\":\"bench\"}}}}}}\n",
            i + 2
        ));
    }
    stdin
        .write_all(body.as_bytes())
        .map_err(|e| e.to_string())?;
    drop(stdin);
    let done = child.wait_with_output().map_err(|e| e.to_string())?;
    if !done.status.success() {
        return Err(format!("mcp exited {:?}", done.status));
    }
    if done.stdout.iter().filter(|b| **b == b'\n').count() < calls + 1 {
        return Err("mcp returned too few lines".into());
    }
    Ok(())
}

fn time_fixtures(root: &Path, get: &Tool) -> Result<(Vec<Row>, String), String> {
    let mut notes = String::new();
    let mut rows = Vec::new();
    for bytes in [256 * 1024, 4 * 1024 * 1024] {
        let path = root.join(format!("fixture-{bytes}.txt"));
        let chunk = vec![b'a'; 1024];
        let mut f = File::create(&path).map_err(|e| e.to_string())?;
        let mut left = bytes;
        while left > 0 {
            let n = left.min(chunk.len());
            f.write_all(&chunk[..n]).map_err(|e| e.to_string())?;
            left -= n;
        }
        f.sync_all().map_err(|e| e.to_string())?;
        let mut samples = Vec::new();
        for _ in 0..10 {
            let start = Instant::now();
            let text = fsx::read_strict(&path)?;
            if text.len() != bytes {
                return Err(format!("fixture len {} != {bytes}", text.len()));
            }
            samples.push(start.elapsed().as_micros());
        }
        rows.push(Row::from_us(
            &format!("read_strict fixture {bytes} bytes"),
            samples,
        ));
        notes.push_str(&format!(
            "- fixture {bytes} bytes on disk: {} bytes\n",
            fs::metadata(&path).map_err(|e| e.to_string())?.len()
        ));
    }
    let body = "word ".repeat(3_000);
    let args = Value::obj(vec![
        ("agent", Value::str("bench")),
        ("kind", Value::str("finding")),
        ("summary", Value::str("large body")),
        ("id", Value::str("large-body")),
        ("body", Value::String(body)),
        (
            "evidence",
            Value::Array(
                (0..40)
                    .map(|i| Value::String(format!("chisel:f{i}.rs")))
                    .collect(),
            ),
        ),
    ]);
    run_tool(tool("libellus_note")?, &args)?;
    let fetch = Value::obj(vec![
        ("agent", Value::str("bench")),
        ("id", Value::str("large-body")),
    ]);
    rows.push(Row::from_us(
        "get note with 40 evidence pointers and a long body",
        time_tool(get, &fetch, 20)?,
    ));
    let path = root.join(".libellus/agents/bench/notes/large-body.md");
    notes.push_str(&format!(
        "- large-body note file: {} bytes\n",
        fs::metadata(&path).map_err(|e| e.to_string())?.len()
    ));
    Ok((rows, notes))
}

fn store_size(store: &Path, scale: usize, with_ledger: bool) -> Result<String, String> {
    let notes = dir_bytes(&store.join("agents/bench/notes"))?;
    let ledger = fs::metadata(store.join("agents/bench/ledger.jsonl"))
        .map(|m| m.len())
        .unwrap_or(0);
    let label = if with_ledger {
        format!("{scale} notes and the ledger")
    } else {
        format!("{scale} notes")
    };
    Ok(format!(
        "- at {label}: notes directory {notes} bytes, ledger file {ledger} bytes, store total {} bytes\n",
        dir_bytes(store)?
    ))
}

fn dir_bytes(dir: &Path) -> Result<u64, String> {
    let mut total = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(cur) = stack.pop() {
        let rd = match fs::read_dir(&cur) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("{}: {e}", cur.display())),
        };
        for entry in rd {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let ft = entry.file_type().map_err(|e| e.to_string())?;
            if ft.is_dir() {
                stack.push(path);
            } else {
                total += entry.metadata().map_err(|e| e.to_string())?.len();
            }
        }
    }
    Ok(total)
}

fn release_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let parent = exe
        .parent()
        .ok_or_else(|| "no parent for the perf binary".to_string())?;
    let candidate = parent.parent().unwrap_or(parent).join("libellus");
    if candidate.is_file() {
        return Ok(candidate.parent().unwrap().to_path_buf());
    }
    if parent.join("libellus").is_file() {
        return Ok(parent.to_path_buf());
    }
    Err(format!("cannot find libellus next to {}", exe.display()))
}

fn machine() -> String {
    let uname = cmd_out("uname", &["-srvm"]);
    let rustc = cmd_out("rustc", &["--version"]);
    let cpu = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let model = cpu
        .lines()
        .find(|l| l.starts_with("model name"))
        .unwrap_or("model name: unknown")
        .replace('\t', " ")
        .to_string();
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let mem = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let total = mem
        .lines()
        .find(|l| l.starts_with("MemTotal:"))
        .unwrap_or("MemTotal: unknown")
        .to_string();
    format!(
        "## Machine\n\n- {uname}\n- {rustc}\n- {model}\n- CPUs: {cpus}\n- {total}\n- host triple: x86_64-unknown-linux-gnu\n"
    )
}

fn cmd_out(bin: &str, args: &[&str]) -> String {
    Command::new(bin)
        .args(args)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_else(|| "unknown".into())
        .trim()
        .to_string()
}
