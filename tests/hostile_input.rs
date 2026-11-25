//! The failure modes found across the other tools: each must be an error
//! reply, with the server still alive for the next request.
mod common;
use common::*;

const PING: &str = "{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"ping\"}\n";

#[test]
fn deeply_nested_json_is_an_error_and_the_server_lives() {
    let dir = project("deep");
    let mut input = Vec::new();
    input.extend(std::iter::repeat_n(b'[', 500_000));
    input.push(b'\n');
    input.extend(PING.as_bytes());
    let replies = talk(&dir, &input);
    assert_eq!(replies.len(), 2, "{replies:?}");
    assert!(replies[0].contains("-32700"), "{}", replies[0]);
    assert!(replies[1].contains("\"id\":9"));
}

#[test]
fn invalid_utf8_is_an_error_and_the_server_lives() {
    let dir = project("utf8");
    let mut input = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"\xff\xfe\"}\n".to_vec();
    input.extend(PING.as_bytes());
    let replies = talk(&dir, &input);
    assert_eq!(replies.len(), 2, "{replies:?}");
    assert!(replies[0].contains("-32700") && replies[0].contains("UTF-8"));
    assert!(replies[1].contains("\"id\":9"));
}

#[test]
fn oversized_line_is_an_error_and_the_server_lives() {
    let dir = project("big");
    let mut input = vec![b'x'; 5 * 1024 * 1024];
    input.push(b'\n');
    input.extend(PING.as_bytes());
    let replies = talk(&dir, &input);
    assert_eq!(replies.len(), 2, "{replies:?}");
    assert!(replies[0].contains("-32600"));
    assert!(replies[1].contains("\"id\":9"));
}

#[test]
fn unknown_methods_are_errors_but_notifications_stay_silent() {
    let dir = project("methods");
    let input = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"resources/list\"}\n{\"jsonrpc\":\"2.0\",\"method\":\"resources/list\"}\n{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n[]\n42\n{\"jsonrpc\":\"2.0\",\"id\":[1],\"method\":\"ping\"}\n";
    let replies = talk(&dir, input.as_bytes());
    assert_eq!(replies.len(), 4, "{replies:?}");
    assert!(replies[0].contains("-32601"));
    assert!(replies[1].contains("batches"));
    assert!(replies[2].contains("-32600"));
    assert!(replies[3].contains("-32600"));
}

#[test]
fn tool_errors_set_is_error() {
    let dir = project("iserror");
    init(&dir);
    let mut input = String::new();
    input += &call(
        "libellus_note",
        r#"{"agent":"main","kind":"question","summary":"s","colour":"red"}"#,
        1,
    );
    input += &call("libellus_note", r#"{"agent":"main","kind":"question"}"#, 2);
    input += &call(
        "libellus_note",
        r#"{"agent":"main","kind":"rumour","summary":"s"}"#,
        3,
    );
    input += &call(
        "libellus_note",
        r#"{"agent":"main","kind":"question","summary":5}"#,
        4,
    );
    input += &call(
        "libellus_spend",
        r#"{"agent":"main","task":"t","tokens_in":-1}"#,
        5,
    );
    input += &call("libellus_spend", r#"{"agent":"main","task":"t"}"#, 6);
    input += &call("libellus_ls", r#""not an object""#, 7);
    let replies = talk(&dir, input.as_bytes());
    assert_eq!(replies.len(), 7);
    for r in &replies {
        assert!(is_error(r), "{r}");
    }
    assert!(replies[0].contains("unknown argument"));
    let unknown_tool = talk(&dir, call("nope", "{}", 1).as_bytes());
    assert!(unknown_tool[0].contains("-32602"));
}

#[test]
fn paths_outside_the_store_are_refused() {
    let dir = project("paths");
    init(&dir);
    let mut input = String::new();
    input += &call(
        "libellus_note",
        r#"{"agent":"main","kind":"finding","summary":"s","id":"../../escape"}"#,
        1,
    );
    input += &call(
        "libellus_note",
        r#"{"agent":"main","kind":"finding","summary":"s","id":"/etc/x"}"#,
        2,
    );
    input += &call(
        "libellus_note",
        r#"{"agent":"../other","kind":"finding","summary":"s"}"#,
        3,
    );
    input += &call(
        "libellus_note",
        r#"{"agent":"main","kind":"finding","summary":"s","id":"a/b/c"}"#,
        4,
    );
    input += &call(
        "libellus_note",
        r#"{"agent":"main","kind":"finding","summary":"s","evidence":["no pointer"]}"#,
        5,
    );
    let replies = talk(&dir, input.as_bytes());
    for r in &replies {
        assert!(is_error(r), "{r}");
    }
    assert!(!dir.parent().unwrap().join("escape.md").exists());
}

#[test]
fn no_store_is_an_error_and_creates_nothing() {
    let dir = project("nostore");
    let replies = talk(
        &dir,
        call(
            "libellus_note",
            r#"{"agent":"main","kind":"finding","summary":"s"}"#,
            1,
        )
        .as_bytes(),
    );
    assert!(
        is_error(&replies[0]) && replies[0].contains("libellus init"),
        "{}",
        replies[0]
    );
    assert!(!dir.join(".libellus").exists());
}
