//! 1.3.0: batch notes and resolves, the shared board (ops, outline, the
//! closing cascade, archive, brief, filters, the HTML page, concurrency,
//! groups), doctor, and the Claude hooks.
mod common;
use common::*;
use libellus::json::{self, Value};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn cmd(dir: &Path, args: &[&str]) -> Command {
    let mut c = Command::new(CLI);
    c.args(args)
        .current_dir(dir)
        .env_remove("LIBELLUS_DIR")
        .env_remove("LIBELLUS_TOOLS")
        .env_remove("LIBELLUS_DISABLE")
        .env_remove("SWARMNOTES_DIR")
        .env("LIBELLUS_AGENT", "main");
    c
}

fn cli(dir: &Path, args: &[&str]) -> Output {
    cmd(dir, args).output().unwrap()
}

fn ok(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn fails(out: &Output) -> String {
    assert!(
        !out.status.success(),
        "expected failure: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn parsed(text: &str) -> Value {
    json::parse(text.trim()).unwrap_or_else(|e| panic!("{e}: {text}"))
}

fn strs(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

fn plan(dir: &Path, ops: &str) -> Value {
    parsed(&ok(&cli(dir, &["plan", "--ops", ops])))
}

fn outline(dir: &Path, text: &str) -> Value {
    parsed(&ok(&cli(dir, &["plan", "--outline", text])))
}

fn task_file(dir: &Path, id: &str) -> String {
    fs::read_to_string(dir.join(".libellus/board").join(format!("{id}.md"))).unwrap()
}

fn status_of(dir: &Path, id: &str) -> String {
    task_file(dir, id)
        .lines()
        .find_map(|l| l.strip_prefix("status: "))
        .unwrap()
        .to_string()
}

fn board_files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir.join(".libellus/board"))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn snapshot(dir: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for name in board_files(dir) {
        let p = dir.join(".libellus/board").join(&name);
        if p.is_file() {
            out.push((name, fs::read_to_string(p).unwrap()));
        }
    }
    out
}

fn note_ids(dir: &Path) -> Vec<String> {
    let v = parsed(&ok(&cli(dir, &["ls"])));
    v.get("notes")
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .map(|n| n.get("id").and_then(Value::as_str).unwrap().to_string())
        .collect()
}

#[test]
fn a_batch_of_notes_is_all_or_nothing() {
    let dir = project("v130-notes");
    init(&dir);
    let out = parsed(&ok(&cli(
        &dir,
        &[
            "note",
            "--notes",
            r#"[{"kind":"decision","summary":"Use a queue"},{"kind":"question","summary":"Does retry apply to PUT?","id":"put"},{"kind":"finding","summary":"Use a queue"}]"#,
        ],
    )));
    assert_eq!(out.get("written").and_then(Value::as_u64), Some(3));
    assert_eq!(
        strs(out.get("ids")),
        vec!["use-a-queue", "put", "use-a-queue-2"]
    );
    let got = parsed(&ok(&cli(&dir, &["get", "--id", "put"])));
    assert_eq!(got.get("status").and_then(Value::as_str), Some("open"));

    // One bad item (and a taken id): nothing is written, and each is named.
    let before = note_ids(&dir);
    let err = fails(&cli(
        &dir,
        &[
            "note",
            "--notes",
            r#"[{"kind":"finding","summary":"fine"},{"kind":"rumour","summary":"x"},{"kind":"finding","summary":7}]"#,
        ],
    ));
    assert!(err.contains("nothing written: 2 of 3 notes"), "{err}");
    assert!(err.contains("notes[1].kind must be one of"), "{err}");
    assert!(err.contains("notes[2].summary must be a string"), "{err}");
    assert_eq!(note_ids(&dir), before);
    let err = fails(&cli(
        &dir,
        &[
            "note",
            "--notes",
            r#"[{"kind":"finding","summary":"fine"},{"kind":"finding","summary":"y","id":"put"},{"kind":"finding","summary":"z","evidence":["no pointer"]},{"kind":"finding","summary":"dup","id":"same"},{"kind":"finding","summary":"dup","id":"same"}]"#,
        ],
    ));
    assert!(err.contains("nothing written: 3 of 5 notes"), "{err}");
    assert!(
        err.contains("notes[1]: note \"put\" already exists"),
        "{err}"
    );
    assert!(err.contains("notes[2]: bad evidence pointer"), "{err}");
    assert!(err.contains("notes[4]: id \"same\" appears twice"), "{err}");
    assert_eq!(note_ids(&dir), before);

    // Unknown keys inside an item are errors too.
    let err = fails(&cli(
        &dir,
        &[
            "note",
            "--notes",
            r#"[{"kind":"finding","summary":"a","colour":"red"}]"#,
        ],
    ));
    assert!(err.contains("notes[0]: unknown key(s) colour"), "{err}");
    // Not both forms at once.
    let err = fails(&cli(
        &dir,
        &[
            "note",
            "--kind",
            "finding",
            "--summary",
            "a",
            "--notes",
            r#"[{"kind":"finding","summary":"b"}]"#,
        ],
    ));
    assert!(err.contains("not both"), "{err}");
    // Over the cap.
    let many: Vec<String> = (0..51)
        .map(|i| format!(r#"{{"kind":"finding","summary":"n{i}"}}"#))
        .collect();
    let err = fails(&cli(
        &dir,
        &["note", "--notes", &format!("[{}]", many.join(","))],
    ));
    assert!(err.contains("at most 50"), "{err}");

    // From a file.
    fs::write(
        dir.join("batch.json"),
        r#"[{"kind":"dead-end","summary":"Global middleware breaks uploads"}]"#,
    )
    .unwrap();
    let out = parsed(&ok(&cli(&dir, &["note", "--notes-file", "batch.json"])));
    assert_eq!(
        strs(out.get("ids")),
        vec!["global-middleware-breaks-uploads"]
    );
    let err = fails(&cli(&dir, &["note", "--notes-file", "missing.json"]));
    assert!(err.contains("does not exist"), "{err}");

    // The seq numbers keep the batch order.
    let ids = note_ids(&dir);
    assert_eq!(&ids[..3], &["use-a-queue", "put", "use-a-queue-2"]);

    // Over MCP, in one call.
    let reply = talk(
        &dir,
        call(
            "libellus_note",
            r#"{"agent":"main","notes":[{"kind":"context","summary":"State one"},{"kind":"context","summary":"State two"}]}"#,
            1,
        )
        .as_bytes(),
    );
    assert!(is_ok(&reply[0]), "{}", reply[0]);
    assert!(reply[0].contains("state-two"), "{}", reply[0]);
}

#[test]
fn a_batch_of_resolves_is_all_or_nothing() {
    let dir = project("v130-resolve");
    init(&dir);
    ok(&cli(
        &dir,
        &[
            "note",
            "--notes",
            r#"[{"kind":"question","summary":"q1","id":"q1"},{"kind":"question","summary":"q2","id":"q2"},{"kind":"decision","summary":"d1","id":"d1"}]"#,
        ],
    ));
    let err = fails(&cli(
        &dir,
        &[
            "resolve",
            "--items",
            r#"[{"id":"q1","outcome":"Yes."},{"id":"nope","outcome":"x"},{"id":"d1","outcome":"again"}]"#,
        ],
    ));
    assert!(err.contains("nothing written"), "{err}");
    assert!(err.contains("items[1]") && err.contains("no note"), "{err}");
    assert!(
        err.contains("items[2]") && err.contains("already resolved"),
        "{err}"
    );
    let q1 = parsed(&ok(&cli(&dir, &["get", "--id", "q1"])));
    assert_eq!(q1.get("status").and_then(Value::as_str), Some("open"));

    let out = parsed(&ok(&cli(
        &dir,
        &[
            "resolve",
            "--items",
            r#"[{"id":"q1","outcome":"Yes."},{"id":"q2","outcome":"No."},{"id":"d1","outcome":"SUPERSEDED: by d2"}]"#,
        ],
    )));
    assert_eq!(out.get("resolved").and_then(Value::as_u64), Some(3));
    let d1 = parsed(&ok(&cli(&dir, &["get", "--id", "d1"])));
    assert_eq!(d1.get("superseded").and_then(Value::as_bool), Some(true));
    let q2 = parsed(&ok(&cli(&dir, &["get", "--id", "q2"])));
    assert!(q2
        .get("body")
        .and_then(Value::as_str)
        .unwrap()
        .contains("Outcome: No."));
    // The single form still works.
    ok(&cli(
        &dir,
        &[
            "note",
            "--kind",
            "question",
            "--summary",
            "q3",
            "--id",
            "q3",
        ],
    ));
    ok(&cli(&dir, &["resolve", "--id", "q3", "--outcome", "Done."]));
    fs::write(
        dir.join("items.json"),
        r#"[{"id":"q3","outcome":"again","supersede":true}]"#,
    )
    .unwrap();
    ok(&cli(&dir, &["resolve", "--items-file", "items.json"]));
}

#[test]
fn board_add_set_and_remove() {
    let dir = project("v130-ops");
    init(&dir);
    let out = plan(
        &dir,
        r#"[{"add":{"title":"Release 1.3","id":"rel"}},{"add":{"title":"Write docs","parent":"rel","owner":"sub-2"}},{"add":{"title":"Typo task","parent":"rel"}}]"#,
    );
    assert_eq!(
        strs(out.get("added")),
        vec!["rel", "write-docs", "typo-task"]
    );
    let docs = task_file(&dir, "write-docs");
    assert!(
        docs.contains("owner: sub-2") && docs.contains("parent: rel"),
        "{docs}"
    );
    assert!(task_file(&dir, "rel").contains("parent:\n"));

    let out = plan(
        &dir,
        r#"[{"set":{"id":"write-docs","status":"doing","title":"Write the docs","outcome":"Half way","evidence":["trammel:plan-3/step-2","adsertio:claim-7"]}},{"remove":"typo-task"}]"#,
    );
    assert_eq!(strs(out.get("removed")), vec!["typo-task"]);
    let docs = task_file(&dir, "write-docs");
    assert!(docs.contains("title: Write the docs"), "{docs}");
    assert!(docs.contains("evidence: [trammel:plan-3/step-2, adsertio:claim-7]"));
    assert!(docs.contains("updated_by: main"));
    assert!(!dir.join(".libellus/board/typo-task.md").exists());

    // Errors write nothing.
    let before = snapshot(&dir);
    for (ops, want) in [
        (r#"[{"remove":"write-docs"}]"#, "only for a todo task"),
        (r#"[{"remove":"rel"}]"#, "has children"),
        (
            r#"[{"set":{"id":"ghost","status":"done"}}]"#,
            "no task \"ghost\"",
        ),
        (r#"[{"add":{"title":"x","id":"rel"}}]"#, "already exists"),
        (r#"[{"add":{"title":"x","parent":"ghost"}}]"#, "no task"),
        (r#"[{"add":{"title":"x","id":"Bad Id"}}]"#, "invalid id"),
        (r#"[{"add":{"title":"x","owner":"children"}}]"#, "reserved"),
        (
            r#"[{"set":{"id":"rel","parent":"write-docs"}}]"#,
            "inside it",
        ),
        (r#"[{"set":{"id":"rel"}}]"#, "at least one"),
        (
            r#"[{"add":{"title":"ok"}},{"set":{"id":"rel","status":"nope"}}]"#,
            "ops[1]",
        ),
        (r#"[{"add":{"title":"a"},"remove":"rel"}]"#, "exactly one"),
        (r#"[{"add":{"title":"a","colour":"red"}}]"#, "unknown key"),
    ] {
        let err = fails(&cli(&dir, &["plan", "--ops", ops]));
        assert!(err.contains(want), "{ops}: {err}");
        assert_eq!(snapshot(&dir), before, "{ops} changed the board");
    }
    // Any agent may change any task; updated_by records who.
    ok(&cli(
        &dir,
        &[
            "plan",
            "--agent",
            "sub-2",
            "--ops",
            r#"[{"set":{"id":"write-docs","status":"blocked"}}]"#,
        ],
    ));
    assert!(task_file(&dir, "write-docs").contains("updated_by: sub-2"));
    // Depth is capped at 6.
    outline(
        &dir,
        "- [ ] L1\n  - [ ] L2\n    - [ ] L3\n      - [ ] L4\n        - [ ] L5\n          - [ ] L6\n",
    );
    let err = fails(&cli(
        &dir,
        &["plan", "--ops", r#"[{"add":{"title":"L7","parent":"l6"}}]"#],
    ));
    assert!(err.contains("at most 6"), "{err}");
}

#[test]
fn outlines_parse_or_name_the_bad_line() {
    let dir = project("v130-outline");
    init(&dir);
    let out = outline(
        &dir,
        "- [ ] Project\n  - [ ] Task\n    - [x] Sub done\n    - [~] Sub doing\n\t- [!] Tabbed blocked\n\n  - [-] Dropped one\n",
    );
    assert_eq!(
        strs(out.get("added")),
        vec![
            "project",
            "task",
            "sub-done",
            "sub-doing",
            "tabbed-blocked",
            "dropped-one"
        ]
    );
    assert_eq!(status_of(&dir, "sub-doing"), "doing");
    assert_eq!(status_of(&dir, "tabbed-blocked"), "blocked");
    assert!(task_file(&dir, "tabbed-blocked").contains("parent: project"));
    assert_eq!(status_of(&dir, "sub-done"), "done");
    // Under a parent, with repeated titles getting -2.
    let out = parsed(&ok(&cli(
        &dir,
        &["plan", "--parent", "task", "--outline", "- [ ] Sub done\n"],
    )));
    assert_eq!(strs(out.get("added")), vec!["sub-done-2"]);
    fs::write(dir.join("o.md"), "- [ ] From file\r\n  - [ ] Child\r\n").unwrap();
    let out = parsed(&ok(&cli(&dir, &["plan", "--outline-file", "o.md"])));
    assert_eq!(strs(out.get("added")), vec!["from-file", "child"]);

    let before = snapshot(&dir);
    for (text, line, want) in [
        ("- [ ] A\n   - [ ] odd\n", 2, "two spaces"),
        ("- [ ] A\n \t- [ ] mixed\n", 2, "not both"),
        ("- [ ] A\n- [X] B\n", 2, "the box is"),
        ("- [ ] A\n- [?] B\n", 2, "the box is"),
        ("* [ ] A\n", 1, "each line is"),
        ("- [ ]A\n", 1, "put a space"),
        ("- [ ] A\n\n- [ ] \n", 3, "title is empty"),
        ("  - [ ] A\n", 1, "first line"),
        ("- [ ] A\n    - [ ] B\n", 2, "more than one level"),
        ("- [ ] A\n  plain text\n", 2, "each line is"),
        (
            "- [ ] 1\n  - [ ] 2\n    - [ ] 3\n      - [ ] 4\n        - [ ] 5\n          - [ ] 6\n            - [ ] 7\n",
            7,
            "deeper than 6",
        ),
        ("- [x] Done parent\n  - [ ] open child\n", 2, ""),
    ] {
        let err = fails(&cli(&dir, &["plan", "--outline", text]));
        if want.is_empty() {
            assert!(err.contains("done-parent") && err.contains("todo or doing"), "{err}");
        } else {
            assert!(err.contains(&format!("outline line {line}:")), "{text:?}: {err}");
            assert!(err.contains(want), "{text:?}: {err}");
        }
        assert_eq!(snapshot(&dir), before, "{text:?} changed the board");
    }
    let err = fails(&cli(&dir, &["plan", "--outline", "\n\n"]));
    assert!(err.contains("no tasks"), "{err}");
    let err = fails(&cli(
        &dir,
        &["plan", "--parent", "ghost", "--outline", "- [ ] A\n"],
    ));
    assert!(
        err.contains("outline line 1") && err.contains("ghost"),
        "{err}"
    );
}

#[test]
fn closing_cascades_up_three_levels_and_archives_the_project() {
    let dir = project("v130-cascade");
    init(&dir);
    outline(
        &dir,
        "- [ ] Proj\n  - [ ] Mid\n    - [ ] Low\n      - [ ] Leaf one\n      - [x] Leaf two\n",
    );
    assert_eq!(status_of(&dir, "low"), "todo");
    let out = plan(
        &dir,
        r#"[{"set":{"id":"leaf-one","status":"done","outcome":"Shipped"}}]"#,
    );
    assert_eq!(
        strs(out.get("closed_by_children")),
        vec!["low", "mid", "proj"]
    );
    let archived = out.get("archived").and_then(Value::as_array).unwrap();
    assert_eq!(archived.len(), 1);
    assert_eq!(
        archived[0].get("project").and_then(Value::as_str),
        Some("proj")
    );
    // Nothing is left on the live board; the history is in the archive.
    assert!(!board_files(&dir).iter().any(|f| f.ends_with(".md")));
    let mid = fs::read_to_string(dir.join(".libellus/board/archive/proj/mid.md")).unwrap();
    assert!(
        mid.contains("status: done") && mid.contains("closed_by: children"),
        "{mid}"
    );
    let leaf = fs::read_to_string(dir.join(".libellus/board/archive/proj/leaf-one.md")).unwrap();
    assert!(leaf.contains("closed_by: main") && leaf.contains("outcome: Shipped"));
    // The brief and the default board do not show it.
    let brief = ok(&cli(&dir, &["brief"]));
    assert!(!brief.contains("leaf-one"), "{brief}");
    assert!(brief.contains("1 archived projects"), "{brief}");
    let b = parsed(&ok(&cli(&dir, &["board"])));
    assert!(!b
        .get("text")
        .and_then(Value::as_str)
        .unwrap()
        .contains("proj"));
    let b = parsed(&ok(&cli(&dir, &["board", "--include_archived"])));
    let text = b.get("text").and_then(Value::as_str).unwrap();
    assert!(
        text.contains("archived proj:") && text.contains("[x] leaf-one"),
        "{text}"
    );
    // An archived task cannot change.
    let err = fails(&cli(
        &dir,
        &[
            "plan",
            "--ops",
            r#"[{"set":{"id":"leaf-one","status":"todo"}}]"#,
        ],
    ));
    assert!(err.contains("archived"), "{err}");
    // A new project with the same id archives beside it.
    outline(&dir, "- [ ] Proj\n  - [ ] Again\n");
    let out = plan(&dir, r#"[{"set":{"id":"again","status":"done"}}]"#);
    let archived = out.get("archived").and_then(Value::as_array).unwrap();
    assert_eq!(
        archived[0].get("folder").and_then(Value::as_str),
        Some("proj-2")
    );
    ok(&cli(&dir, &["check"]));
}

#[test]
fn reopening_a_child_reopens_parents_closed_by_children() {
    let dir = project("v130-reopen");
    init(&dir);
    outline(
        &dir,
        "- [ ] Proj\n  - [ ] Area\n    - [ ] Part\n      - [ ] Bit\n  - [ ] Other\n  - [ ] Keep\n",
    );
    let out = plan(&dir, r#"[{"set":{"id":"bit","status":"done"}}]"#);
    assert_eq!(strs(out.get("closed_by_children")), vec!["part", "area"]);
    assert_eq!(status_of(&dir, "proj"), "todo");
    let out = plan(&dir, r#"[{"set":{"id":"bit","status":"doing"}}]"#);
    assert_eq!(strs(out.get("reopened")), vec!["part", "area"]);
    assert_eq!(status_of(&dir, "area"), "doing");
    assert!(!task_file(&dir, "area").contains("closed"));
    // Adding an open child under a parent closed by children reopens it too.
    plan(&dir, r#"[{"set":{"id":"bit","status":"done"}}]"#);
    let out = plan(&dir, r#"[{"add":{"title":"More","parent":"part"}}]"#);
    assert_eq!(strs(out.get("reopened")), vec!["part", "area"]);
    // A parent an agent closed is not reopened by a child: that is an error.
    plan(&dir, r#"[{"set":{"id":"more","status":"done"}}]"#);
    plan(
        &dir,
        r#"[{"add":{"title":"Done by hand","parent":"other","status":"done"}}]"#,
    );
    assert_eq!(status_of(&dir, "other"), "done");
    plan(&dir, r#"[{"set":{"id":"other","status":"done"}}]"#);
    let before = snapshot(&dir);
    // "other" closed by children, so this works ...
    plan(&dir, r#"[{"set":{"id":"done-by-hand","status":"todo"}}]"#);
    assert_eq!(status_of(&dir, "other"), "doing");
    plan(&dir, r#"[{"set":{"id":"done-by-hand","status":"done"}}]"#);
    // ... but once an agent marks it done itself, reopening a child is refused.
    plan(
        &dir,
        r#"[{"set":{"id":"other","status":"todo"}},{"set":{"id":"other","status":"done"}}]"#,
    );
    assert!(task_file(&dir, "other").contains("closed_by: main"));
    let mid = snapshot(&dir);
    let err = fails(&cli(
        &dir,
        &[
            "plan",
            "--ops",
            r#"[{"set":{"id":"done-by-hand","status":"todo"}}]"#,
        ],
    ));
    assert!(err.contains("todo or doing first"), "{err}");
    assert_eq!(snapshot(&dir), mid);
    assert_ne!(before, mid);
}

#[test]
fn done_with_open_children_is_an_error_and_drop_cascades_down() {
    let dir = project("v130-drop");
    init(&dir);
    outline(
        &dir,
        "- [ ] Proj\n  - [ ] Keep\n  - [ ] Feature\n    - [x] Spike\n    - [ ] Build\n      - [~] Code\n    - [!] Ship\n",
    );
    let before = snapshot(&dir);
    let err = fails(&cli(
        &dir,
        &[
            "plan",
            "--ops",
            r#"[{"set":{"id":"feature","status":"done"}}]"#,
        ],
    ));
    assert!(err.contains("open children: build, ship"), "{err}");
    assert_eq!(snapshot(&dir), before);
    // Closing the children in the same call, in any order, is fine.
    let dir2 = project("v130-drop-2");
    init(&dir2);
    outline(&dir2, "- [ ] P\n  - [ ] A\n    - [ ] B\n  - [ ] C\n");
    plan(
        &dir2,
        r#"[{"set":{"id":"a","status":"done"}},{"set":{"id":"b","status":"done"}}]"#,
    );
    assert_eq!(status_of(&dir2, "a"), "done");

    let out = plan(
        &dir,
        r#"[{"set":{"id":"feature","status":"dropped","outcome":"Out of scope"}}]"#,
    );
    assert_eq!(strs(out.get("dropped")), vec!["build", "code", "ship"]);
    assert_eq!(status_of(&dir, "spike"), "done");
    assert_eq!(status_of(&dir, "code"), "dropped");
    assert_eq!(status_of(&dir, "proj"), "todo");
    // The last open sibling done: proj closes (one done child is enough).
    let out = plan(&dir, r#"[{"set":{"id":"keep","status":"done"}}]"#);
    assert_eq!(strs(out.get("closed_by_children")), vec!["proj"]);
    // All children dropped closes nothing.
    let dir3 = project("v130-drop-3");
    init(&dir3);
    outline(&dir3, "- [ ] P\n  - [ ] A\n  - [ ] B\n");
    let out = plan(
        &dir3,
        r#"[{"set":{"id":"a","status":"dropped"}},{"set":{"id":"b","status":"dropped"}}]"#,
    );
    assert!(out.get("closed_by_children").is_none());
    assert_eq!(status_of(&dir3, "p"), "todo");
}

#[test]
fn the_brief_lists_my_open_tasks_with_paths() {
    let dir = project("v130-brief");
    init(&dir);
    outline(
        &dir,
        "- [ ] Proj\n  - [ ] Api\n    - [ ] Write tests\n    - [~] Add route\n    - [!] Wait for keys\n  - [x] Setup\n",
    );
    plan(
        &dir,
        r#"[{"add":{"title":"Docs","parent":"proj","owner":"sub-2"}},{"add":{"title":"Docs index","parent":"docs","owner":"sub-2","status":"doing"}}]"#,
    );
    let brief = ok(&cli(&dir, &["brief"]));
    let board = brief.split("## Board\n").nth(1).unwrap();
    let board: Vec<&str> = board.lines().take_while(|l| !l.is_empty()).collect();
    assert_eq!(
        board,
        vec![
            "- [~] proj > api > add-route: Add route",
            "- [!] proj > api > wait-for-keys: Wait for keys",
            "- [ ] proj > api > write-tests: Write tests",
            "- board: 4 todo, 2 doing, 1 blocked, 1 done, 0 dropped, 0 archived projects",
        ],
        "{brief}"
    );
    assert!(brief.find("## Board").unwrap() < brief.find("## Open questions").unwrap());
    assert!(brief.contains("tick off a task with libellus_plan"));
    // A sub-agent sees its own tasks and the parents they belong to.
    let sub = ok(&cli(&dir, &["brief", "--agent", "sub-2"]));
    assert!(
        sub.contains("- [~] proj > docs > docs-index: Docs index"),
        "{sub}"
    );
    assert!(!sub.contains("add-route"), "{sub}");
    // Capped by max_items.
    let short = ok(&cli(&dir, &["brief", "--max_items", "1"]));
    assert!(short.contains("Showing 1 of your 3 open tasks"), "{short}");
    assert!(!short.contains("write-tests"));
    let json = parsed(&ok(&cli(&dir, &["--version"])));
    assert_eq!(json.get("version").and_then(Value::as_str), Some("1.3.0"));
    // The MCP brief has the fields.
    let reply = talk(
        &dir,
        call("libellus_brief", r#"{"agent":"main"}"#, 1).as_bytes(),
    );
    assert!(reply[0].contains("\\\"open_mine\\\":3"), "{}", reply[0]);
}

#[test]
fn board_filters_keep_the_output_small() {
    let dir = project("v130-filters");
    init(&dir);
    outline(
        &dir,
        "- [ ] Alpha\n  - [ ] A1\n  - [x] A2\n- [ ] Beta\n  - [ ] B1\n",
    );
    plan(&dir, r#"[{"set":{"id":"b1","owner":"sub-1"}}]"#);
    let all = parsed(&ok(&cli(&dir, &["board"])));
    let text = all.get("text").and_then(Value::as_str).unwrap();
    assert_eq!(
        text,
        "- [ ] alpha: Alpha\n  - [ ] a1: A1\n- [ ] beta: Beta\n  - [ ] b1: B1 @sub-1\n"
    );
    let counts = all.get("counts").unwrap();
    assert_eq!(counts.get("todo").and_then(Value::as_u64), Some(4));
    assert_eq!(counts.get("done").and_then(Value::as_u64), Some(1));
    let closed = parsed(&ok(&cli(&dir, &["board", "--open_only=false"])));
    assert!(closed
        .get("text")
        .and_then(Value::as_str)
        .unwrap()
        .contains("[x] a2: A2"));
    let mine = parsed(&ok(&cli(&dir, &["board", "--owner", "sub-1"])));
    assert_eq!(
        mine.get("text").and_then(Value::as_str).unwrap(),
        "- [ ] beta: Beta\n  - [ ] b1: B1 @sub-1\n"
    );
    let alpha = parsed(&ok(&cli(&dir, &["board", "--project", "alpha"])));
    assert_eq!(
        alpha.get("text").and_then(Value::as_str).unwrap(),
        "- [ ] alpha: Alpha\n  - [ ] a1: A1\n"
    );
    let err = fails(&cli(&dir, &["board", "--project", "a1"]));
    assert!(err.contains("no open project"), "{err}");
    let err = fails(&cli(&dir, &["board", "--colour", "red"]));
    assert!(err.contains("unknown flag"), "{err}");
}

#[test]
fn the_html_page_is_escaped_self_contained_and_read_only() {
    let dir = project("v130-html");
    init(&dir);
    ok(&cli(
        &dir,
        &[
            "note",
            "--kind",
            "question",
            "--summary",
            "Is <b>this</b> & \"that\" safe?",
        ],
    ));
    outline(
        &dir,
        "- [ ] Site <script>alert(1)</script>\n  - [ ] Copy & \"quotes\" 'single'\n  - [x] Done one\n- [ ] Old\n  - [ ] Old child\n",
    );
    plan(
        &dir,
        r#"[{"set":{"id":"done-one","outcome":"<img src=x onerror=alert(1)>","evidence":["adsertio:claim-<7>"]}},{"set":{"id":"old-child","status":"done"}}]"#,
    );
    let page = fs::read_to_string(dir.join(".libellus/board.html")).unwrap();
    assert!(page.starts_with("<!doctype html>"));
    assert!(!page.contains("<script"), "a script tag got through");
    assert!(!page.contains("<img"), "markup got through");
    assert!(page.contains("Site &lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(page.contains("Copy &amp; &quot;quotes&quot; &#39;single&#39;"));
    assert!(page.contains("Is &lt;b&gt;this&lt;/b&gt; &amp; &quot;that&quot; safe?"));
    assert!(page.contains("Read-only."));
    assert!(page.contains("review task &lt;id&gt;"));
    for col in ["todo (", "doing (", "blocked (", "done ("] {
        assert!(page.contains(col), "missing column {col}");
    }
    assert!(page.contains("type=\"checkbox\" disabled checked"));
    assert!(page.contains("prefers-color-scheme:dark"));
    assert!(page.contains("name=\"viewport\""));
    assert!(page.contains("1/3 done"), "per project progress");
    assert!(
        page.contains("Archived projects") && page.contains("<details><summary><code>old</code>")
    );
    assert!(!page.contains("http://") && !page.contains("https://"));
    assert!(!page.contains("src=\"") && !page.contains("<link"));
    let store = dir.join(".libellus").display().to_string();
    assert!(
        page.contains(&libellus::html::esc(&store)),
        "store path in the footer"
    );
    // --html on the CLI, to a chosen file.
    let out = parsed(&ok(&cli(&dir, &["board", "--html", "--out", "copy.html"])));
    assert!(out
        .get("html")
        .and_then(Value::as_str)
        .unwrap()
        .ends_with("copy.html"));
    let copy = fs::read_to_string(dir.join("copy.html")).unwrap();
    assert!(copy.contains("Site &lt;script&gt;"));
    let err = fails(&cli(&dir, &["board", "--html", "--owner", "main"]));
    assert!(err.contains("only --out"), "{err}");
}

#[test]
fn ticking_siblings_at_once_closes_the_parent_exactly_once() {
    let dir = project("v130-race");
    init(&dir);
    let n = 8;
    let mut text = String::from("- [ ] Proj\n  - [ ] Keep open\n  - [ ] Parent\n");
    for i in 0..n {
        text.push_str(&format!("    - [ ] Kid {i}\n"));
    }
    outline(&dir, &text);
    let gate = std::sync::Arc::new(std::sync::Barrier::new(n));
    let handles: Vec<_> = (0..n)
        .map(|i| {
            let dir = dir.clone();
            let gate = gate.clone();
            std::thread::spawn(move || {
                let ops = format!(r#"[{{"set":{{"id":"kid-{i}","status":"done"}}}}]"#);
                let mut c = cmd(
                    &dir,
                    &["plan", "--agent", &format!("sub-{i}"), "--ops", &ops],
                );
                gate.wait();
                c.output().unwrap()
            })
        })
        .collect();
    let mut closes = 0;
    for h in handles {
        let out = h.join().unwrap();
        let v = parsed(&ok(&out));
        if strs(v.get("closed_by_children")).contains(&"parent".to_string()) {
            closes += 1;
        }
    }
    assert_eq!(closes, 1, "the parent closed more than once, or never");
    for i in 0..n {
        let f = task_file(&dir, &format!("kid-{i}"));
        assert!(f.contains("status: done"), "lost update on kid-{i}: {f}");
        assert!(f.contains(&format!("updated_by: sub-{i}")));
    }
    assert!(task_file(&dir, "parent").contains("closed_by: children"));
    assert_eq!(status_of(&dir, "proj"), "todo");
    assert!(strays(&dir).is_empty(), "{:?}", strays(&dir));
    ok(&cli(&dir, &["check"]));
}

#[test]
fn the_board_group_can_be_switched_off() {
    let dir = project("v130-groups");
    init(&dir);
    fs::write(
        dir.join("AGENTS.md"),
        "```agent-tools\nlibellus: disable board\n```\n",
    )
    .unwrap();
    let input = format!(
        "{}{}",
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n",
        call(
            "libellus_plan",
            r#"{"agent":"main","outline":"- [ ] A\n"}"#,
            2
        )
    );
    let replies = talk(&dir, input.as_bytes());
    assert!(!replies[0].contains("libellus_plan") && !replies[0].contains("libellus_board"));
    assert!(replies[0].contains("libellus_doctor"));
    assert!(
        is_error(&replies[1]) && replies[1].contains("switched off"),
        "{}",
        replies[1]
    );
    let brief = ok(&cli(&dir, &["brief"]));
    assert!(!brief.contains("## Board"), "{brief}");
    // On by default, with its schema.
    fs::remove_file(dir.join("AGENTS.md")).unwrap();
    let replies = talk(
        &dir,
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n",
    );
    assert!(replies[0].contains("libellus_plan") && replies[0].contains("\"maxItems\":100"));
    let replies = talk_env(
        &dir,
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n",
        &[("LIBELLUS_DISABLE", "board")],
    );
    assert!(!replies[0].contains("libellus_plan"));
}

#[test]
fn doctor_is_healthy_then_names_a_planted_problem() {
    let dir = project("v130-doctor");
    init(&dir);
    outline(&dir, "- [ ] P\n  - [ ] T\n");
    let out = cli(&dir, &["doctor"]);
    let v = parsed(&ok(&out));
    assert_eq!(v.get("ok").and_then(Value::as_bool), Some(true), "{v:?}");
    let names: Vec<String> = v
        .get("checks")
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .map(|c| c.get("name").and_then(Value::as_str).unwrap().to_string())
        .collect();
    for want in [
        "version",
        "store",
        "writable",
        "store_files",
        "stray_files",
        "tool_groups",
        "mcp_binary",
        "relatum",
    ] {
        assert!(
            names.contains(&want.to_string()),
            "missing {want}: {names:?}"
        );
    }
    assert!(strays(&dir).is_empty());

    // A damaged task file: exit 1, the check names it and says how to fix it.
    fs::write(
        dir.join(".libellus/board/t.md"),
        "---\ntitle: T\nstatus: maybe\n---\n",
    )
    .unwrap();
    let out = cli(&dir, &["doctor"]);
    assert_eq!(out.status.code(), Some(1));
    let v = parsed(&String::from_utf8_lossy(&out.stdout));
    assert_eq!(v.get("ok").and_then(Value::as_bool), Some(false));
    let bad = v
        .get("checks")
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .find(|c| c.get("name").and_then(Value::as_str) == Some("store_files"))
        .unwrap()
        .clone();
    assert_eq!(bad.get("ok").and_then(Value::as_bool), Some(false));
    assert!(bad
        .get("detail")
        .and_then(Value::as_str)
        .unwrap()
        .contains("t.md"));
    assert!(bad
        .get("fix")
        .and_then(Value::as_str)
        .unwrap()
        .contains("by hand"));
    // check covers the board too.
    let err = fails(&cli(&dir, &["check"]));
    assert!(err.contains("corrupt board task t"), "{err}");
    // And a board write refuses to work on a damaged board.
    let err = fails(&cli(&dir, &["plan", "--outline", "- [ ] X\n"]));
    assert!(err.contains("corrupt board task t"), "{err}");
    fs::remove_file(dir.join(".libellus/board/t.md")).unwrap();

    // A bad AGENTS.md line fails tool_groups with a fix.
    fs::write(
        dir.join("AGENTS.md"),
        "```agent-tools\nlibellus: disable ledgr\n```\n",
    )
    .unwrap();
    let out = cli(&dir, &["doctor"]);
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        text.contains("unknown group") && text.contains("agent-tools block"),
        "{text}"
    );
    fs::remove_file(dir.join("AGENTS.md")).unwrap();

    // A lock left by a process that is gone: doctor reports it and leaves
    // it; --fix removes it.
    #[cfg(unix)]
    {
        let lock = dir.join(".libellus/board/.board.lock");
        fs::write(&lock, "pid 999999 x\n").unwrap();
        let out = cli(&dir, &["doctor"]);
        assert_eq!(out.status.code(), Some(1));
        assert!(lock.exists(), "doctor without --fix changed the store");
        assert!(String::from_utf8_lossy(&out.stdout).contains("doctor --fix"));
        let out = cli(&dir, &["doctor", "--fix"]);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
        assert!(!lock.exists());
    }
    // No store at all.
    let empty = project("v130-doctor-empty");
    let out = cli(&empty, &["doctor"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stdout).contains("libellus init"));
    // Over MCP, in core.
    let reply = talk(&dir, call("libellus_doctor", "{}", 1).as_bytes());
    assert!(
        is_ok(&reply[0]) && reply[0].contains("store_files"),
        "{}",
        reply[0]
    );
}

fn hook_run(dir: &Path, args: &[&str], stdin: &str) -> Output {
    let mut c = cmd(dir, args);
    c.env_remove("LIBELLUS_AGENT")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = c.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn the_claude_hooks_are_valid_json_and_run_without_a_shell() {
    let dir = project("v130-hook");
    init(&dir);
    let text = ok(&cli(&dir, &["hook", "--agent", "main", "--host", "claude"]));
    let body: String = text
        .lines()
        .filter(|l| !l.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    let v = parsed(&body);
    let hooks = v.get("hooks").unwrap();
    let start = &hooks.get("SessionStart").and_then(Value::as_array).unwrap()[0];
    assert_eq!(
        start.get("matcher").and_then(Value::as_str),
        Some("startup|resume|compact")
    );
    let h = &start.get("hooks").and_then(Value::as_array).unwrap()[0];
    assert_eq!(h.get("command").and_then(Value::as_str), Some("libellus"));
    assert_eq!(
        strs(h.get("args")),
        vec!["hook-run", "session-start", "--agent", "main"]
    );
    let sub = &hooks
        .get("SubagentStart")
        .and_then(Value::as_array)
        .unwrap()[0];
    let h = &sub.get("hooks").and_then(Value::as_array).unwrap()[0];
    assert_eq!(strs(h.get("args")), vec!["hook-run", "subagent-start"]);
    for bad in ["&&", "||", "~", "$", "%"] {
        assert!(!body.contains(bad), "shell syntax {bad} in {body}");
    }

    // The main agent plans work for a sub-agent by its hook name.
    plan(
        &dir,
        r#"[{"add":{"title":"Proj","id":"proj"}},{"add":{"title":"Review the parser","parent":"proj","owner":"explore-abc123"}}]"#,
    );
    let out = hook_run(
        &dir,
        &["hook-run", "subagent-start"],
        r#"{"session_id":"s","hook_event_name":"SubagentStart","agent_id":"agent-abc123","agent_type":"Explore"}"#,
    );
    assert!(out.status.success());
    let v = parsed(&String::from_utf8_lossy(&out.stdout));
    let spec = v.get("hookSpecificOutput").unwrap();
    assert_eq!(
        spec.get("hookEventName").and_then(Value::as_str),
        Some("SubagentStart")
    );
    let ctx = spec
        .get("additionalContext")
        .and_then(Value::as_str)
        .unwrap();
    assert!(
        ctx.contains("Your Libellus agent name is explore-abc123"),
        "{ctx}"
    );
    assert!(ctx.contains("# Libellus brief for explore-abc123"), "{ctx}");
    assert!(ctx.contains("proj > review-the-parser"), "{ctx}");

    let out = hook_run(
        &dir,
        &["hook-run", "session-start", "--agent", "main"],
        "{}",
    );
    let v = parsed(&String::from_utf8_lossy(&out.stdout));
    let spec = v.get("hookSpecificOutput").unwrap();
    assert_eq!(
        spec.get("hookEventName").and_then(Value::as_str),
        Some("SessionStart")
    );
    assert!(spec
        .get("additionalContext")
        .and_then(Value::as_str)
        .unwrap()
        .starts_with("# Libellus brief for main"));

    // Bad input never breaks the session: exit 0, nothing on stdout, a note on stderr.
    for (args, input) in [
        (vec!["hook-run", "subagent-start"], "not json"),
        (vec!["hook-run", "subagent-start"], "[1,2]"),
        (vec!["hook-run", "subagent-start"], "{}"),
        (vec!["hook-run", "nope"], ""),
        (vec!["hook-run"], ""),
        (vec!["hook-run", "session-start"], ""),
    ] {
        let out = hook_run(&dir, &args, input);
        assert_eq!(out.status.code(), Some(0), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("libellus hook-run"));
    }
    let nostore = project("v130-hook-nostore");
    let out = hook_run(
        &nostore,
        &["hook-run", "session-start", "--agent", "main"],
        "",
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
}
