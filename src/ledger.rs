//! The number ledger: one JSON object per line, append-only, per agent.
//!
//! Tokens and cost are whole numbers the agent reports (Libellus never calls a
//! model and never guesses a price). Cost is in millionths of a US dollar so
//! no float ever touches the sums. A line that is not JSON, or that has an
//! unknown field, fails the read and names the line. A line whose agent, task
//! or timestamp is wrong is left out of the totals and named in `warnings`.
//! A torn last line is ignored on read and blocks the next append.

use crate::fsx;
use crate::json::{self, Value};
use crate::store::Store;
use std::io::Write;

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub ts: String,
    pub agent: String,
    pub task: String,
    pub step: Option<String>,
    pub model: Option<String>,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cost_micro_usd: u64,
    pub note: Option<String>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Totals {
    pub entries: u64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cost_micro_usd: u64,
}

impl Totals {
    fn add(&mut self, e: &Entry) -> Result<(), String> {
        let over = || "ledger totals overflowed a 64-bit number".to_string();
        let tokens_in = self.tokens_in.checked_add(e.tokens_in).ok_or_else(over)?;
        let tokens_out = self.tokens_out.checked_add(e.tokens_out).ok_or_else(over)?;
        let cost_micro_usd = self
            .cost_micro_usd
            .checked_add(e.cost_micro_usd)
            .ok_or_else(over)?;
        self.tokens_in = tokens_in;
        self.tokens_out = tokens_out;
        self.cost_micro_usd = cost_micro_usd;
        self.entries += 1;
        Ok(())
    }

    pub fn to_value(&self) -> Value {
        Value::obj(vec![
            ("entries", Value::uint(self.entries)),
            ("tokens_in", Value::uint(self.tokens_in)),
            ("tokens_out", Value::uint(self.tokens_out)),
            ("cost_micro_usd", Value::uint(self.cost_micro_usd)),
        ])
    }
}

impl Entry {
    pub fn to_line(&self) -> String {
        let mut p = vec![
            ("ts", Value::str(&self.ts)),
            ("agent", Value::str(&self.agent)),
            ("task", Value::str(&self.task)),
        ];
        if let Some(s) = &self.step {
            p.push(("step", Value::str(s)));
        }
        if let Some(m) = &self.model {
            p.push(("model", Value::str(m)));
        }
        p.push(("tokens_in", Value::uint(self.tokens_in)));
        p.push(("tokens_out", Value::uint(self.tokens_out)));
        p.push(("cost_micro_usd", Value::uint(self.cost_micro_usd)));
        if let Some(n) = &self.note {
            p.push(("note", Value::str(n)));
        }
        Value::obj(p).to_json()
    }

    fn from_line(line: &str) -> Result<Entry, String> {
        let v = json::parse(line).map_err(|e| e.to_string())?;
        let Value::Object(pairs) = &v else {
            return Err("not a JSON object".into());
        };
        const KEYS: &[&str] = &[
            "ts",
            "agent",
            "task",
            "step",
            "model",
            "tokens_in",
            "tokens_out",
            "cost_micro_usd",
            "note",
        ];
        if let Some((k, _)) = pairs.iter().find(|(k, _)| !KEYS.contains(&k.as_str())) {
            return Err(format!("unknown field \"{k}\""));
        }
        let s = |k: &str| v.get(k).and_then(Value::as_str).map(String::from);
        let need = |k: &str| s(k).ok_or_else(|| format!("missing or non-string \"{k}\""));
        let n = |k: &str| match v.get(k) {
            Some(x) => x
                .as_u64()
                .ok_or_else(|| format!("\"{k}\" must be a whole number")),
            None => Err(format!("missing \"{k}\"")),
        };
        Ok(Entry {
            ts: need("ts")?,
            agent: need("agent")?,
            task: need("task")?,
            step: s("step"),
            model: s("model"),
            tokens_in: n("tokens_in")?,
            tokens_out: n("tokens_out")?,
            cost_micro_usd: n("cost_micro_usd")?,
            note: s("note"),
        })
    }

    /// Why this line must not be added into `agent`'s totals.
    fn rejected(&self, agent: &str) -> Option<String> {
        if self.agent != agent || !fsx::valid_slug(&self.agent) {
            return Some(format!(
                "agent \"{}\" does not match \"{agent}\"",
                self.agent
            ));
        }
        if !fsx::valid_slug(&self.task) {
            return Some(format!("task \"{}\" is not a slug", self.task));
        }
        if let Some(step) = &self.step {
            if !fsx::valid_slug(step) {
                return Some(format!("step \"{step}\" is not a slug"));
            }
        }
        if !fsx::valid_timestamp(&self.ts) {
            return Some(format!(
                "timestamp \"{}\" is not YYYY-MM-DDTHH:MM:SSZ",
                self.ts
            ));
        }
        None
    }
}

/// One ledger file after a single read. `entries` are the lines that count.
/// `warnings` name lines that were left out. `torn` is a partial last line.
pub struct Read {
    pub entries: Vec<Entry>,
    pub warnings: Vec<String>,
    pub torn: bool,
}

/// Largest ledger file we will read, and the longest single line.
pub const MAX_LEDGER_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_LEDGER_LINE: usize = 64 * 1024;

/// Append one entry as a single `write` of a whole line.
///
/// The ledger is locked for the one read, the overflow check, and the write.
/// The new totals are checked before any byte is appended. On overflow, or
/// when the file ends in a torn line, the file is left unchanged. The
/// returned entries are that one read plus this entry, so the count is taken
/// under the lock.
pub fn append(store: &Store, entry: &Entry) -> Result<Vec<Entry>, String> {
    let path = store.ledger_path(&entry.agent)?;
    if let Some(dir) = path.parent() {
        fsx::make_private_dir(dir)?;
    }
    let _lock = fsx::Lock::acquire(&path)?;
    fsx::refuse_special(&path)?;
    let existing = read(store, &entry.agent)?;
    if existing.torn {
        return Err(format!(
            "not appended: {} ends in a partial line. Finish that line with a newline, or delete it. The file was not changed.",
            path.display()
        ));
    }
    let (so_far, bad) = sum_prefix(&existing.entries);
    if let Some(line) = bad {
        return Err(format!(
            "not appended: {} line {line}: ledger totals overflowed a 64-bit number. The file was not changed.",
            path.display()
        ));
    }
    let mut totals = so_far;
    if totals.add(entry).is_err() {
        return Err(format!(
            "not appended: adding this entry to {} would overflow a 64-bit total. The file was not changed.",
            path.display()
        ));
    }
    let mut line = entry.to_line();
    line.push('\n');
    let mut f = fsx::open_append_private(&path)?;
    f.write_all(line.as_bytes())
        .map_err(|e| format!("cannot append to {}: {e}", path.display()))?;
    f.sync_all()
        .map_err(|e| format!("cannot sync {}: {e}", path.display()))?;
    let mut entries = existing.entries;
    entries.push(entry.clone());
    Ok(entries)
}

/// A ledger read, summed. `overflow_line` is the first entry that does not fit.
pub struct Summed {
    pub entries: Vec<Entry>,
    pub totals: Totals,
    pub overflow_line: Option<usize>,
    pub warnings: Vec<String>,
}

/// Read a ledger and sum what fits. A corrupt line is still a hard error.
/// An overflow is reported as `overflow_line` (1-based) and the entries stop
/// before that line. `warnings` are lines left out of the totals.
pub fn read_summed(store: &Store, agent: &str) -> Result<Summed, String> {
    let read = read(store, agent)?;
    let (totals, overflow_line) = sum_prefix(&read.entries);
    let entries = match overflow_line {
        Some(line) => read.entries[..line - 1].to_vec(),
        None => read.entries,
    };
    Ok(Summed {
        entries,
        totals,
        overflow_line,
        warnings: read.warnings,
    })
}

pub fn read(store: &Store, agent: &str) -> Result<Read, String> {
    let path = store.ledger_path(agent)?;
    let text = match fsx::file_kind(&path)? {
        fsx::FileKind::Missing => {
            return Ok(Read {
                entries: Vec::new(),
                warnings: Vec::new(),
                torn: false,
            })
        }
        fsx::FileKind::Symlink => {
            return Err(format!(
                "{} is a symlink; refusing to follow it",
                path.display()
            ))
        }
        fsx::FileKind::Other => {
            return Err(format!(
                "{} is not a regular file; refusing to open it",
                path.display()
            ))
        }
        fsx::FileKind::Regular { len } if len > MAX_LEDGER_BYTES => {
            return Err(format!(
                "ledger {} is {len} bytes, over the {MAX_LEDGER_BYTES} byte cap",
                path.display()
            ))
        }
        fsx::FileKind::Regular { .. } => fsx::read_strict(&path)?,
    };
    parse_text(&path, agent, &text)
}

fn parse_text(path: &std::path::Path, agent: &str, text: &str) -> Result<Read, String> {
    let torn = !text.is_empty() && !text.ends_with('\n');
    let mut lines: Vec<&str> = text.lines().collect();
    if torn {
        lines.pop();
    }
    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    if torn {
        warnings.push(format!(
            "ledger {} ends in a partial line. Finish that line with a newline, or delete it. It was not counted.",
            path.display()
        ));
    }
    for (i, line) in lines.iter().enumerate() {
        if line.len() > MAX_LEDGER_LINE {
            return Err(format!(
                "ledger {} line {} is {} bytes, over the {MAX_LEDGER_LINE} byte line cap",
                path.display(),
                i + 1,
                line.len()
            ));
        }
        let entry = Entry::from_line(line)
            .map_err(|e| format!("corrupt ledger {} line {}: {e}", path.display(), i + 1))?;
        if let Some(why) = entry.rejected(agent) {
            warnings.push(format!(
                "ledger {} line {}: {why} It was not counted.",
                path.display(),
                i + 1
            ));
            continue;
        }
        entries.push(entry);
    }
    Ok(Read {
        entries,
        warnings,
        torn,
    })
}

pub fn total(entries: &[Entry]) -> Result<Totals, String> {
    let (totals, bad) = sum_prefix(entries);
    match bad {
        Some(line) => Err(format!(
            "ledger totals overflowed a 64-bit number at line {line}"
        )),
        None => Ok(totals),
    }
}

/// Totals of the leading entries that fit in 64 bits. `overflow_line` is the
/// 1-based index of the first entry that does not fit, when there is one.
pub fn sum_prefix(entries: &[Entry]) -> (Totals, Option<usize>) {
    let mut totals = Totals::default();
    for (i, entry) in entries.iter().enumerate() {
        if totals.add(entry).is_err() {
            return (totals, Some(i + 1));
        }
    }
    (totals, None)
}

/// Totals grouped by task, sorted by task name.
pub fn by_task(entries: &[Entry]) -> Result<Vec<(String, Totals)>, String> {
    grouped(entries, |e| e.task.clone())
}

/// Totals grouped by step, sorted by step name. A missing step is `""`.
pub fn by_step(entries: &[Entry]) -> Result<Vec<(String, Totals)>, String> {
    grouped(entries, |e| e.step.clone().unwrap_or_default())
}

fn grouped(
    entries: &[Entry],
    key_of: impl Fn(&Entry) -> String,
) -> Result<Vec<(String, Totals)>, String> {
    let mut out: Vec<(String, Totals)> = Vec::new();
    for e in entries {
        let key = key_of(e);
        match out.iter_mut().find(|(k, _)| *k == key) {
            Some((_, t)) => t.add(e)?,
            None => {
                let mut t = Totals::default();
                t.add(e)?;
                out.push((key, t));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(task: &str, i: u64) -> Entry {
        Entry {
            ts: "t".into(),
            agent: "main".into(),
            task: task.into(),
            step: None,
            model: None,
            tokens_in: i,
            tokens_out: 1,
            cost_micro_usd: 10,
            note: None,
        }
    }

    #[test]
    fn lines_round_trip_and_sum() {
        let x = e("a", 5);
        assert_eq!(Entry::from_line(&x.to_line()).unwrap(), x);
        let t = total(&[e("a", 1), e("b", 2)]).unwrap();
        assert_eq!((t.entries, t.tokens_in, t.cost_micro_usd), (2, 3, 20));
        assert_eq!(
            by_task(&[e("b", 1), e("a", 1), e("a", 1)]).unwrap()[0]
                .1
                .entries,
            2
        );
        let mut stepped = e("retry", 4);
        stepped.step = Some("plan".into());
        let rows = by_step(&[stepped, e("retry", 1)]).unwrap();
        assert_eq!(rows[0].0, "");
        assert_eq!(rows[0].1.tokens_in, 1);
        assert_eq!(rows[1].0, "plan");
        assert_eq!(rows[1].1.tokens_in, 4);
    }

    #[test]
    fn overflow_is_an_error() {
        assert!(total(&[e("a", u64::MAX), e("a", 1)]).is_err());
    }

    #[test]
    fn damaged_lines_are_errors() {
        assert!(Entry::from_line("{").is_err());
        assert!(Entry::from_line(r#"{"ts":"t","agent":"a","task":"b","tokens_in":1,"tokens_out":1,"cost_micro_usd":1,"x":1}"#).is_err());
        assert!(Entry::from_line(
            r#"{"ts":"t","agent":"a","task":"b","tokens_in":-1,"tokens_out":1,"cost_micro_usd":1}"#
        )
        .is_err());
    }
}
