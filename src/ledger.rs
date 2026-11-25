//! The number ledger: one JSON object per line, append-only, per agent.
//!
//! Tokens and cost are whole numbers the agent reports (Libellus never calls a
//! model and never guesses a price). Cost is in millionths of a US dollar so
//! no float ever touches the sums. Reading is strict: a damaged line fails
//! the read and names the line, it is never skipped.

use crate::fsx;
use crate::json::{self, Value};
use crate::store::Store;
use std::fs;
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
        self.entries += 1;
        self.tokens_in = self.tokens_in.checked_add(e.tokens_in).ok_or_else(over)?;
        self.tokens_out = self.tokens_out.checked_add(e.tokens_out).ok_or_else(over)?;
        self.cost_micro_usd = self
            .cost_micro_usd
            .checked_add(e.cost_micro_usd)
            .ok_or_else(over)?;
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
}

/// Append one entry as a single `write` of a whole line.
pub fn append(store: &Store, entry: &Entry) -> Result<(), String> {
    let path = store.ledger_path(&entry.agent)?;
    if let Some(dir) = path.parent() {
        fsx::make_private_dir(dir)?;
    }
    // Refuse to extend a ledger that is already damaged.
    read(store, &entry.agent)?;
    let mut line = entry.to_line();
    line.push('\n');
    let mut f = fsx::open_append_private(&path)?;
    f.write_all(line.as_bytes())
        .map_err(|e| format!("cannot append to {}: {e}", path.display()))?;
    f.sync_all()
        .map_err(|e| format!("cannot sync {}: {e}", path.display()))
}

pub fn read(store: &Store, agent: &str) -> Result<Vec<Entry>, String> {
    let path = store.ledger_path(agent)?;
    let text = match fs::metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        _ => fsx::read_strict(&path)?,
    };
    if !text.is_empty() && !text.ends_with('\n') {
        return Err(format!(
            "{} ends in a partial line; refusing to read past it",
            path.display()
        ));
    }
    text.lines()
        .enumerate()
        .map(|(i, l)| {
            Entry::from_line(l)
                .map_err(|e| format!("corrupt ledger {} line {}: {e}", path.display(), i + 1))
        })
        .collect()
}

pub fn total(entries: &[Entry]) -> Result<Totals, String> {
    let mut t = Totals::default();
    for e in entries {
        t.add(e)?;
    }
    Ok(t)
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
