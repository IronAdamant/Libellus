//! What an agent needs: pages of notes, errors that say the next step, and a
//! brief that still fits the old headings.
mod common;
use common::*;
use std::process::Command;

fn cli(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(CLI)
        .args(args)
        .current_dir(dir)
        .env_remove("LIBELLUS_DIR")
        .output()
        .unwrap()
}

fn note(dir: &std::path::Path, id: &str) {
    let out = cli(
        dir,
        &[
            "note",
            "--agent",
            "main",
            "--kind",
            "finding",
            "--id",
            id,
            "--summary",
            id,
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn ls_offset_pages_the_newest_notes_and_keeps_total() {
    let dir = project("ux-page");
    init(&dir);
    for id in ["n1", "n2", "n3", "n4", "n5"] {
        note(&dir, id);
    }
    let page = |limit: &str, offset: &str| {
        let out = cli(
            &dir,
            &[
                "ls", "--agent", "main", "--limit", limit, "--offset", offset,
            ],
        );
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    };
    let first = page("2", "0");
    assert!(
        first.contains("\"total\":5") && first.contains("\"returned\":2"),
        "{first}"
    );
    assert!(
        first.contains("n4") && first.contains("n5") && !first.contains("n3"),
        "{first}"
    );
    let second = page("2", "2");
    assert!(
        second.contains("\"offset\":2") && second.contains("n2") && second.contains("n3"),
        "{second}"
    );
    assert!(!second.contains("n5") && !second.contains("n1"), "{second}");
    let last = page("2", "4");
    assert!(
        last.contains("n1") && !last.contains("n2") && last.contains("\"returned\":1"),
        "{last}"
    );
    let none = page("2", "5");
    assert!(
        none.contains("\"returned\":0") && none.contains("\"total\":5"),
        "{none}"
    );
}

#[test]
fn errors_name_the_next_call_and_the_brief_ends_with_next() {
    let dir = project("ux-errors");
    init(&dir);
    note(&dir, "pick");
    let again = cli(
        &dir,
        &[
            "note",
            "--agent",
            "main",
            "--kind",
            "finding",
            "--id",
            "pick",
            "--summary",
            "other",
        ],
    );
    let err = String::from_utf8_lossy(&again.stderr);
    assert!(
        err.contains("already exists") && err.contains("libellus_get"),
        "{err}"
    );

    let missing = cli(&dir, &["get", "--agent", "main", "--id", "nope"]);
    let err = String::from_utf8_lossy(&missing.stderr);
    assert!(
        err.contains("no note") && err.contains("libellus_ls"),
        "{err}"
    );

    let open = cli(
        &dir,
        &[
            "note",
            "--agent",
            "main",
            "--kind",
            "question",
            "--id",
            "q1",
            "--summary",
            "Still open?",
        ],
    );
    assert!(
        open.status.success(),
        "{}",
        String::from_utf8_lossy(&open.stderr)
    );
    let done = cli(
        &dir,
        &[
            "resolve",
            "--agent",
            "main",
            "--id",
            "q1",
            "--outcome",
            "Yes.",
        ],
    );
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    let twice = cli(
        &dir,
        &[
            "resolve",
            "--agent",
            "main",
            "--id",
            "q1",
            "--outcome",
            "Again.",
        ],
    );
    let err = String::from_utf8_lossy(&twice.stderr);
    assert!(
        err.contains("already resolved")
            && err.contains("not changed")
            && err.contains("libellus_get"),
        "{err}"
    );

    let brief = cli(&dir, &["brief", "--agent", "main"]);
    let text = String::from_utf8(brief.stdout).unwrap();
    assert!(text.starts_with("# Libellus brief for main\n"), "{text}");
    assert!(text.contains("## Open questions (0)"));
    assert!(
        !text.contains("Next:"),
        "nothing to resolve or share: {text}"
    );
    let ask = cli(
        &dir,
        &[
            "note",
            "--agent",
            "main",
            "--kind",
            "question",
            "--id",
            "q2",
            "--summary",
            "Another?",
        ],
    );
    assert!(
        ask.status.success(),
        "{}",
        String::from_utf8_lossy(&ask.stderr)
    );
    let brief = cli(&dir, &["brief", "--agent", "main"]);
    let text = String::from_utf8(brief.stdout).unwrap();
    assert!(
        text.contains("Next: resolve an open question with libellus_resolve."),
        "{text}"
    );
    assert!(!text.contains("libellus_spend"), "{text}");
}

#[test]
fn unknown_ls_field_is_still_an_error() {
    let dir = project("ux-unknown");
    init(&dir);
    let out = cli(&dir, &["ls", "--agent", "main", "--colour", "red"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown flag"));
}
