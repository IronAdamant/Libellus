//! The project is std only: no dependencies, no dev-dependencies, and exactly
//! one package (this one) in Cargo.lock.

use std::fs;

fn section(toml: &str, name: &str) -> Vec<String> {
    let header = format!("[{name}]");
    let mut inside = false;
    let mut lines = Vec::new();
    for line in toml.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            inside = t == header;
            continue;
        }
        if inside && !t.is_empty() && !t.starts_with('#') {
            lines.push(t.to_string());
        }
    }
    lines
}

#[test]
fn cargo_toml_has_no_dependencies() {
    let toml = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();
    for name in ["dependencies", "dev-dependencies", "build-dependencies"] {
        assert!(
            section(&toml, name).is_empty(),
            "[{name}] must stay empty: {:?}",
            section(&toml, name)
        );
    }
    assert!(
        !toml.contains("[target."),
        "no target-specific dependencies"
    );
}

#[test]
fn cargo_lock_has_one_package() {
    let lock = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.lock")).unwrap();
    assert_eq!(
        lock.matches("[[package]]").count(),
        1,
        "Cargo.lock must hold only this crate"
    );
}
