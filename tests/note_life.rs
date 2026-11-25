//! The life of one note: read it back, then mark it resolved.
mod common;
use common::*;
use std::fs;
use std::process::Command;

fn cli(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(CLI)
        .args(args)
        .current_dir(dir)
        .env_remove("LIBELLUS_DIR")
        .output()
        .unwrap()
}

fn write_question(dir: &std::path::Path, body: &str) {
    let args = [
        "note",
        "--agent",
        "main",
        "--kind",
        "question",
        "--id",
        "retry/post",
        "--summary",
        "Does retry apply to POST?",
        "--body",
        body,
        "--evidence",
        "trammel:plan-3/step-2",
    ];
    let out = cli(dir, &args);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn get_returns_the_note_including_a_long_body() {
    let dir = project("get-ok");
    init(&dir);
    let body = format!("detail {}\n", "word ".repeat(900));
    write_question(&dir, &body);
    let r = talk(
        &dir,
        call("libellus_get", r#"{"agent":"main","id":"retry/post"}"#, 1).as_bytes(),
    );
    assert!(is_ok(&r[0]), "{}", r[0]);
    assert!(r[0].contains("Does retry apply to POST?"), "{}", r[0]);
    assert!(r[0].contains("trammel:plan-3/step-2"), "{}", r[0]);
    assert!(
        r[0].contains("word word") && !r[0].contains("truncated"),
        "get returns the whole body: {}",
        r[0].len()
    );
    let out = cli(&dir, &["get", "--agent", "main", "--id", "retry/post"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("\"status\":\"open\"") && text.contains("word word"));
}

#[test]
fn get_of_a_missing_or_corrupt_note_is_an_error() {
    let dir = project("get-bad");
    init(&dir);
    let missing = talk(
        &dir,
        call("libellus_get", r#"{"agent":"main","id":"nope"}"#, 1).as_bytes(),
    );
    assert!(
        is_error(&missing[0]) && missing[0].contains("no note"),
        "{}",
        missing[0]
    );
    write_question(&dir, "The client retries every method.");
    let path = dir.join(".libellus/agents/main/notes/retry/post.md");
    let before = fs::read(&path).unwrap();
    fs::write(&path, b"---\nkind: nope\n").unwrap();
    let r = talk(
        &dir,
        call("libellus_get", r#"{"agent":"main","id":"retry/post"}"#, 1).as_bytes(),
    );
    assert!(is_error(&r[0]) && r[0].contains("corrupt note"), "{}", r[0]);
    fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
    let r = talk(
        &dir,
        call("libellus_get", r#"{"agent":"main","id":"retry/post"}"#, 1).as_bytes(),
    );
    assert!(is_error(&r[0]) && r[0].contains("UTF-8"), "{}", r[0]);
    fs::write(&path, before).unwrap();
    let mut text = fs::read_to_string(&path).unwrap();
    text = text.replace("agent: main", "agent: other");
    fs::write(&path, text).unwrap();
    let r = talk(
        &dir,
        call("libellus_get", r#"{"agent":"main","id":"retry/post"}"#, 1).as_bytes(),
    );
    assert!(
        is_error(&r[0]) && r[0].contains("belongs to agent"),
        "{}",
        r[0]
    );
}

#[cfg(unix)]
#[test]
fn get_refuses_a_symlink_note() {
    let dir = project("get-link");
    init(&dir);
    write_question(&dir, "body");
    let path = dir.join(".libellus/agents/main/notes/retry/post.md");
    let outside = dir.join("outside.md");
    fs::rename(&path, &outside).unwrap();
    std::os::unix::fs::symlink(&outside, &path).unwrap();
    let r = talk(
        &dir,
        call("libellus_get", r#"{"agent":"main","id":"retry/post"}"#, 1).as_bytes(),
    );
    assert!(is_error(&r[0]) && r[0].contains("symlink"), "{}", r[0]);
}

#[test]
fn resolve_appends_once_keeps_created_and_a_second_resolve_fails() {
    let dir = project("resolve-ok");
    init(&dir);
    write_question(&dir, "The client retries every method.");
    let path = dir.join(".libellus/agents/main/notes/retry/post.md");
    let before = fs::read_to_string(&path).unwrap();
    let created = before
        .lines()
        .find(|l| l.starts_with("created:"))
        .unwrap()
        .to_string();
    let r = talk(
        &dir,
        call(
            "libellus_resolve",
            r#"{"agent":"main","id":"retry/post","outcome":"No, GET only."}"#,
            1,
        )
        .as_bytes(),
    );
    assert!(is_ok(&r[0]), "{}", r[0]);
    assert!(r[0].contains(r#"\"status\":\"resolved\""#), "{}", r[0]);
    let after = fs::read_to_string(&path).unwrap();
    assert!(after.contains(&created), "created changed:\n{after}");
    assert!(after.contains("The client retries every method."));
    assert_eq!(
        after.matches("Outcome: No, GET only.").count(),
        1,
        "{after}"
    );
    assert!(after.starts_with("---\n") && after.trim_end().ends_with("Outcome: No, GET only."));
    assert!(note_parses(&after));
    assert!(strays(&dir).is_empty(), "{:?}", strays(&dir));

    let again = talk(
        &dir,
        call(
            "libellus_resolve",
            r#"{"agent":"main","id":"retry/post","outcome":"Still no."}"#,
            2,
        )
        .as_bytes(),
    );
    assert!(
        is_error(&again[0]) && again[0].contains("already resolved"),
        "{}",
        again[0]
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), after);
    assert!(strays(&dir).is_empty());

    let brief = talk(
        &dir,
        call("libellus_brief", r#"{"agent":"main"}"#, 3).as_bytes(),
    );
    assert!(
        is_ok(&brief[0]) && brief[0].contains("Open questions (0)"),
        "{}",
        brief[0]
    );
}

#[test]
fn resolve_rejects_a_bad_outcome_and_a_foreign_note() {
    let dir = project("resolve-bad");
    init(&dir);
    write_question(&dir, "body");
    let path = dir.join(".libellus/agents/main/notes/retry/post.md");
    let before = fs::read(&path).unwrap();
    let mut input = String::new();
    input += &call(
        "libellus_resolve",
        r#"{"agent":"main","id":"retry/post","outcome":"one\ntwo"}"#,
        1,
    );
    input += &call(
        "libellus_resolve",
        &format!(
            r#"{{"agent":"main","id":"retry/post","outcome":"{}"}}"#,
            "x".repeat(201)
        ),
        2,
    );
    input += &call(
        "libellus_resolve",
        r#"{"agent":"main","id":"retry/post","outcome":"  padded"}"#,
        3,
    );
    input += &call(
        "libellus_resolve",
        r#"{"agent":"main","id":"retry/post","outcome":""}"#,
        4,
    );
    input += &call(
        "libellus_resolve",
        r#"{"agent":"other","id":"retry/post","outcome":"no"}"#,
        5,
    );
    input += &call(
        "libellus_resolve",
        r#"{"agent":"main","id":"../../etc/passwd","outcome":"no"}"#,
        6,
    );
    input += &call(
        "libellus_get",
        r#"{"agent":"main","id":"retry/post","colour":"red"}"#,
        7,
    );
    input += &call(
        "libellus_resolve",
        r#"{"agent":"main","id":"missing","outcome":"no"}"#,
        8,
    );
    let r = talk(&dir, input.as_bytes());
    assert_eq!(r.len(), 8, "{r:?}");
    for line in &r {
        assert!(is_error(line), "{line}");
    }
    assert!(r[0].contains("one line"), "{}", r[0]);
    assert!(r[1].contains("200"), "{}", r[1]);
    assert_eq!(
        fs::read(&path).unwrap(),
        before,
        "a refused resolve must not write"
    );
    assert!(strays(&dir).is_empty());
    assert!(!dir.join("etc/passwd").exists());

    let out = cli(
        &dir,
        &[
            "resolve",
            "--agent",
            "main",
            "--id",
            "retry/post",
            "--colour",
            "red",
            "--outcome",
            "no",
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown argument"));
    let out = cli(&dir, &["get", "--agent", "main", "--id"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn resolve_of_exactly_200_characters_and_the_cli_round_trip() {
    let dir = project("resolve-cli");
    init(&dir);
    write_question(&dir, "body");
    let outcome = "é".repeat(200);
    let out = cli(
        &dir,
        &[
            "resolve",
            "--agent",
            "main",
            "--id",
            "retry/post",
            "--outcome",
            &outcome,
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = fs::read_to_string(dir.join(".libellus/agents/main/notes/retry/post.md")).unwrap();
    assert!(text.contains(&format!("Outcome: {outcome}")));
    assert!(text.contains("status: resolved"));
    assert!(strays(&dir).is_empty());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(dir.join(".libellus/agents/main/notes/retry/post.md"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}

/// The file is still a whole note: frontmatter closed, then the body.
fn note_parses(text: &str) -> bool {
    let rest = text.strip_prefix("---\n").unwrap_or("");
    let Some((head, body)) = rest.split_once("\n---\n") else {
        return false;
    };
    head.lines().any(|l| l == "status: resolved")
        && head.lines().all(|l| !l.is_empty() && l.contains(':'))
        && body.contains("Outcome:")
}
