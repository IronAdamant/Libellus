//! A damaged store fails loudly. It is never read as empty and never reset.
mod common;
use common::*;
use std::fs;
use std::process::Command;

fn note(id: u32, summary: &str, kind: &str) -> String {
    call(
        "libellus_note",
        &format!(r#"{{"agent":"main","kind":"{kind}","summary":"{summary}"}}"#),
        id,
    )
}

#[test]
fn notes_ledger_and_brief_round_trip() {
    let dir = project("roundtrip");
    init(&dir);
    let mut input = String::new();
    input += &note(1, "Does retry apply to POST?", "question");
    input += &note(2, "Use backoff with jitter", "decision");
    input += &note(3, "Linear backoff caused herds", "dead-end");
    input += &call(
        "libellus_note",
        r#"{"agent":"main","kind":"finding","summary":"Flaky test","evidence":["chisel:tests/net.rs","probatio:runs/7"],"tags":["net"],"body":"Details here."}"#,
        4,
    );
    input += &call(
        "libellus_spend",
        r#"{"agent":"main","task":"retry","step":"plan","tokens_in":1000,"tokens_out":200,"cost_micro_usd":15000}"#,
        5,
    );
    input += &call(
        "libellus_spend",
        r#"{"agent":"main","task":"retry","tokens_in":10}"#,
        6,
    );
    input += &call(
        "libellus_ls",
        r#"{"agent":"main","kind":"question","status":"open"}"#,
        7,
    );
    input += &call(
        "libellus_ls",
        r#"{"agent":"main","bodies":true,"kind":"finding"}"#,
        8,
    );
    input += &call("libellus_brief", r#"{"agent":"main"}"#, 9);
    input += &call("libellus_ls", r#"{"agent":"other"}"#, 10);
    let r = talk(&dir, input.as_bytes());
    assert_eq!(r.len(), 10);
    for x in &r {
        assert!(is_ok(x), "{x}");
    }
    assert!(r[6].contains("Does retry apply to POST?") && !r[6].contains("backoff"));
    assert!(r[7].contains("Details here.") && r[7].contains("chisel:tests/net.rs"));
    assert!(
        r[8].contains("Open questions (1)")
            && r[8].contains("1010 in, 200 out tokens, 15000 micro-USD (2 ledger entries)"),
        "{}",
        r[8]
    );
    assert!(
        r[9].contains("\\\"total\\\":0"),
        "private to the agent: {}",
        r[9]
    );
    assert!(strays(&dir).is_empty());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &str| fs::metadata(dir.join(p)).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(".libellus/agents/main/ledger.jsonl"), 0o600);
        assert_eq!(
            mode(".libellus/agents/main/notes/does-retry-apply-to-post.md"),
            0o600
        );
        assert_eq!(mode(".libellus/agents/main/notes"), 0o700);
    }
}

#[test]
fn an_existing_note_is_never_overwritten() {
    let dir = project("nooverwrite");
    init(&dir);
    let a = call(
        "libellus_note",
        r#"{"agent":"main","kind":"decision","summary":"first","id":"pick"}"#,
        1,
    );
    let b = call(
        "libellus_note",
        r#"{"agent":"main","kind":"decision","summary":"second","id":"pick"}"#,
        2,
    );
    let r = talk(&dir, format!("{a}{b}").as_bytes());
    assert!(
        is_ok(&r[0]) && is_error(&r[1]) && r[1].contains("already exists"),
        "{r:?}"
    );
    let text = fs::read_to_string(dir.join(".libellus/agents/main/notes/pick.md")).unwrap();
    assert!(text.contains("summary: first"));
    assert!(strays(&dir).is_empty());
}

#[test]
fn a_corrupt_note_is_an_error_not_an_empty_list() {
    let dir = project("corruptnote");
    init(&dir);
    talk(&dir, note(1, "fine", "finding").as_bytes());
    let path = dir.join(".libellus/agents/main/notes/fine.md");
    let before = fs::read_to_string(&path).unwrap();
    fs::write(&path, before.replace("kind: finding", "kind: ???")).unwrap();
    let mut input = call("libellus_ls", r#"{"agent":"main"}"#, 1);
    input += &call("libellus_brief", r#"{"agent":"main"}"#, 2);
    let r = talk(&dir, input.as_bytes());
    assert!(is_error(&r[0]) && r[0].contains("corrupt note"), "{}", r[0]);
    assert!(is_error(&r[1]));
    let out = Command::new(CLI)
        .arg("check")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    fs::write(&path, [0xffu8, 0xfe, 0x00]).unwrap();
    let r = talk(
        &dir,
        call("libellus_ls", r#"{"agent":"main"}"#, 1).as_bytes(),
    );
    assert!(is_error(&r[0]) && r[0].contains("UTF-8"), "{}", r[0]);
}

#[test]
fn a_corrupt_ledger_is_an_error_and_is_not_extended() {
    let dir = project("corruptledger");
    init(&dir);
    talk(
        &dir,
        call(
            "libellus_spend",
            r#"{"agent":"main","task":"t","tokens_in":5}"#,
            1,
        )
        .as_bytes(),
    );
    let path = dir.join(".libellus/agents/main/ledger.jsonl");
    let mut text = fs::read_to_string(&path).unwrap();
    text.push_str("{not json\n");
    fs::write(&path, &text).unwrap();
    let r = talk(
        &dir,
        call(
            "libellus_spend",
            r#"{"agent":"main","task":"t","tokens_in":5}"#,
            1,
        )
        .as_bytes(),
    );
    assert!(is_error(&r[0]) && r[0].contains("line 2"), "{}", r[0]);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        text,
        "a damaged ledger must be left as it was"
    );
    let r = talk(
        &dir,
        call("libellus_brief", r#"{"agent":"main"}"#, 1).as_bytes(),
    );
    assert!(is_error(&r[0]));
}

#[test]
fn concurrent_writers_lose_nothing() {
    let dir = project("concurrent");
    init(&dir);
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let dir = dir.clone();
            std::thread::spawn(move || {
                for j in 0..5 {
                    let out = Command::new(CLI)
                        .args([
                            "note",
                            "--agent",
                            "main",
                            "--kind",
                            "finding",
                            "--summary",
                            &format!("note {i}-{j}"),
                        ])
                        .current_dir(&dir)
                        .output()
                        .unwrap();
                    assert!(
                        out.status.success(),
                        "{}",
                        String::from_utf8_lossy(&out.stderr)
                    );
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let out = Command::new(CLI)
        .args(["ls", "--agent", "main", "--limit", "1000"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("\"total\":40"));
    assert!(strays(&dir).is_empty());
}
