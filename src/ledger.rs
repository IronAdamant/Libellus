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
use crate::ledger_meta;
use crate::store::Store;
use std::collections::HashMap;
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
    pub(crate) fn add(&mut self, e: &Entry) -> Result<(), String> {
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
        let opt = |k: &str| match v.get(k) {
            None => Ok(None),
            Some(x) => x
                .as_str()
                .map(|t| Some(t.to_string()))
                .ok_or_else(|| format!("\"{k}\" must be a string")),
        };
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
            step: opt("step")?,
            model: opt("model")?,
            tokens_in: n("tokens_in")?,
            tokens_out: n("tokens_out")?,
            cost_micro_usd: n("cost_micro_usd")?,
            note: opt("note")?,
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
/// `lines` holds the 1-based file line of each entry, so a message about an
/// entry names the line in the file, not its place among the counted ones.
/// `warnings` name lines that were left out. `torn` is a partial last line.
pub struct Read {
    pub entries: Vec<Entry>,
    pub lines: Vec<usize>,
    pub warnings: Vec<String>,
    pub torn: bool,
}

/// The first counted entry whose total does not fit in 64 bits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Overflow {
    /// Index into `Read::entries`. The entries before it fit.
    pub index: usize,
    /// 1-based line in the ledger file.
    pub line: usize,
}

impl Read {
    /// Totals of the leading entries that fit, and where the first one that
    /// does not fit sits, both as an entry index and as a file line. Every
    /// overflow message goes through here so the line number cannot drift.
    pub fn sum(&self) -> (Totals, Option<Overflow>) {
        let (totals, index) = sum_prefix(&self.entries);
        let overflow = index.map(|index| Overflow {
            index,
            line: self.lines.get(index).copied().unwrap_or(index + 1),
        });
        (totals, overflow)
    }
}

/// The one wording for a ledger whose running total overflows at `line`.
pub fn overflow_text(path: &std::path::Path, line: usize) -> String {
    format!(
        "ledger {} line {line}: totals overflowed a 64-bit number",
        path.display()
    )
}

/// Largest ledger file we will read, and the longest single line.
pub const MAX_LEDGER_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_LEDGER_LINE: usize = 64 * 1024;

/// Append one entry as a single `write` of a whole line.
///
/// The ledger is locked for the read, the overflow check, and the write.
/// The new totals are checked before any byte is appended. A spend that
/// would pass the 32 MiB cap is refused with the file unchanged. On a failed
/// write, a partial line from this call is truncated back off. When
/// `ledger.meta` still matches the file length and the last line, the totals
/// are taken from that cache instead of parsing the file again.
pub fn append(store: &Store, entry: &Entry) -> Result<Recorded, String> {
    let path = store.ledger_path(&entry.agent)?;
    if let Some(dir) = path.parent() {
        fsx::make_private_dir(dir)?;
    }
    let _lock = fsx::Lock::acquire(&path)?;
    fsx::refuse_special(&path)?;
    let mut line = entry.to_line();
    line.push('\n');
    if line.len() > MAX_LEDGER_LINE {
        return Err(format!(
            "not appended: this line is {} bytes, over the {MAX_LEDGER_LINE} byte line cap. The file was not changed.",
            line.len()
        ));
    }
    let prev_len = match fsx::file_kind(&path)? {
        fsx::FileKind::Missing => 0,
        fsx::FileKind::Regular { len } => len,
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
    };
    if prev_len > MAX_LEDGER_BYTES || prev_len + line.len() as u64 > MAX_LEDGER_BYTES {
        return Err(format!(
            "not appended: {} is {prev_len} bytes and this line is {} bytes, which would pass the {MAX_LEDGER_BYTES} byte cap. The file was not changed. Archive it: libellus_ledger with archive true (libellus ledger --agent {} --archive).",
            path.display(),
            line.len(),
            entry.agent
        ));
    }
    let (agent_totals, task_totals) = match ledger_meta::matching(&path, prev_len)? {
        Some(cache) => {
            let mut agent_totals = cache.totals.clone();
            if agent_totals.add(entry).is_err() {
                return Err(overflow_msg(&path));
            }
            let (task_totals, tasks, tasks_complete) = if cache.tasks_complete {
                let mut task_totals = cache.tasks.get(&entry.task).cloned().unwrap_or_default();
                if task_totals.add(entry).is_err() {
                    return Err(overflow_msg(&path));
                }
                let mut tasks = cache.tasks;
                tasks.insert(entry.task.clone(), task_totals.clone());
                let complete = tasks.len() <= ledger_meta::MAX_CACHED_TASKS;
                if !complete {
                    tasks.clear();
                }
                (task_totals, tasks, complete)
            } else {
                let mut task_totals = sum_task(&path, &entry.agent, &entry.task)?;
                if task_totals.add(entry).is_err() {
                    return Err(overflow_msg(&path));
                }
                (task_totals, HashMap::new(), false)
            };
            write_line(&path, prev_len, line.as_bytes())?;
            let saved = ledger_meta::Cache {
                len: prev_len + line.len() as u64,
                totals: agent_totals.clone(),
                tasks,
                tasks_complete,
                last_hash: ledger_meta::hash_bytes(line.as_bytes()),
            };
            let _ = ledger_meta::save(&path, &saved);
            (agent_totals, task_totals)
        }
        None => append_from_file(store, &path, entry, prev_len, &line)?,
    };
    Ok(Recorded {
        agent: agent_totals,
        task: task_totals,
    })
}

/// Totals for one task, without keeping every entry. Used when the cache
/// does not hold the task map.
fn sum_task(path: &std::path::Path, agent: &str, task: &str) -> Result<Totals, String> {
    let text = fsx::read_strict(path)?;
    if !text.is_empty() && !text.ends_with('\n') {
        return Err(format!(
            "not appended: {} ends in a partial line. Finish that line with a newline, or delete it. The file was not changed.",
            path.display()
        ));
    }
    let mut totals = Totals::default();
    for (i, line) in text.lines().enumerate() {
        if line.len() > MAX_LEDGER_LINE {
            return Err(format!(
                "ledger {} line {} is {} bytes, over the {MAX_LEDGER_LINE} byte line cap",
                path.display(),
                i + 1,
                line.len()
            ));
        }
        let row = Entry::from_line(line)
            .map_err(|e| format!("corrupt ledger {} line {}: {e}", path.display(), i + 1))?;
        if row.rejected(agent).is_some() || row.task != task {
            continue;
        }
        totals.add(&row)?;
    }
    Ok(totals)
}

fn overflow_msg(path: &std::path::Path) -> String {
    format!(
        "not appended: adding this entry to {} would overflow a 64-bit total. The file was not changed.",
        path.display()
    )
}

fn append_from_file(
    store: &Store,
    path: &std::path::Path,
    entry: &Entry,
    prev_len: u64,
    line: &str,
) -> Result<(Totals, Totals), String> {
    let existing = read(store, &entry.agent)?;
    if existing.torn {
        return Err(format!(
            "not appended: {} ends in a partial line. Finish that line with a newline, or delete it. The file was not changed.",
            path.display()
        ));
    }
    let (so_far, bad) = existing.sum();
    if let Some(over) = bad {
        return Err(format!(
            "not appended: {}. The file was not changed.",
            overflow_text(path, over.line)
        ));
    }
    let mut agent_totals = so_far;
    if agent_totals.add(entry).is_err() {
        return Err(overflow_msg(path));
    }
    let mut task_totals = Totals::default();
    for old in existing.entries.iter().filter(|e| e.task == entry.task) {
        task_totals.add(old)?;
    }
    task_totals.add(entry)?;
    write_line(path, prev_len, line.as_bytes())?;
    let mut tasks = task_map(&existing.entries)?;
    tasks.insert(entry.task.clone(), task_totals.clone());
    let tasks_complete = tasks.len() <= ledger_meta::MAX_CACHED_TASKS;
    if !tasks_complete {
        tasks.clear();
    }
    let saved = ledger_meta::Cache {
        len: prev_len + line.len() as u64,
        totals: agent_totals.clone(),
        tasks,
        tasks_complete,
        last_hash: ledger_meta::hash_bytes(line.as_bytes()),
    };
    let _ = ledger_meta::save(path, &saved);
    Ok((agent_totals, task_totals))
}

fn task_map(entries: &[Entry]) -> Result<HashMap<String, Totals>, String> {
    let mut tasks: HashMap<String, Totals> = HashMap::new();
    for entry in entries {
        let slot = tasks.entry(entry.task.clone()).or_default();
        slot.add(entry)?;
    }
    Ok(tasks)
}

/// Write `bytes` at the end of `path`. On a failed write, truncate back to
/// `prev_len` so this call does not leave its own torn line.
fn write_line(path: &std::path::Path, prev_len: u64, bytes: &[u8]) -> Result<(), String> {
    ignore_file_size_signal();
    let mut file = fsx::open_append_private(path)?;
    if let Err(e) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        let rolled = file.set_len(prev_len).and_then(|_| file.sync_all());
        let extra = match rolled {
            Ok(()) => " The partial line was removed.",
            Err(re) => {
                return Err(format!(
                    "cannot append to {}: {e}. The partial line could not be removed: {re}",
                    path.display()
                ));
            }
        };
        return Err(format!("cannot append to {}: {e}.{extra}", path.display()));
    }
    Ok(())
}

#[cfg(unix)]
fn ignore_file_size_signal() {
    // SIGXFSZ is 25 on Linux and macOS. Ignoring it turns a file-size limit
    // into EFBIG, which `write_line` can roll back, instead of killing us.
    extern "C" {
        fn signal(sig: i32, handler: *const ()) -> *const ();
    }
    unsafe {
        signal(25, 1usize as *const ());
    }
}

#[cfg(not(unix))]
fn ignore_file_size_signal() {}

/// A ledger read, summed. `overflow_line` is the 1-based file line of the
/// first entry that does not fit.
pub struct Summed {
    pub entries: Vec<Entry>,
    pub totals: Totals,
    pub overflow_line: Option<usize>,
    pub warnings: Vec<String>,
    pub torn: bool,
}

impl Summed {
    fn empty() -> Summed {
        Summed {
            entries: Vec::new(),
            totals: Totals::default(),
            overflow_line: None,
            warnings: Vec::new(),
            torn: false,
        }
    }
}

/// What `append` recorded. Totals include the line just written.
pub struct Recorded {
    pub agent: Totals,
    pub task: Totals,
}

/// Read a ledger and sum what fits. A corrupt line is still a hard error.
/// An overflow is reported as `overflow_line` (1-based file line) and the
/// entries stop before that line. `warnings` are lines left out of the totals.
pub fn read_summed(store: &Store, agent: &str) -> Result<Summed, String> {
    let read = read(store, agent)?;
    let (totals, overflow) = read.sum();
    let mut entries = read.entries;
    if let Some(over) = overflow {
        entries.truncate(over.index);
    }
    Ok(Summed {
        entries,
        totals,
        overflow_line: overflow.map(|over| over.line),
        warnings: read.warnings,
        torn: read.torn,
    })
}

/// Read one agent's ledger under its lock and refresh `ledger.meta` when the
/// file is a clean sum. Brief and ledger use this so the next spend can skip
/// the parse. A hard read error is returned as-is; callers that must stay up
/// (the brief) turn it into `ledger_error`.
pub fn load_report(store: &Store, agent: &str) -> Result<Summed, String> {
    let path = store.ledger_path(agent)?;
    if matches!(fsx::file_kind(&path)?, fsx::FileKind::Missing) {
        return Ok(Summed::empty());
    }
    let _lock = fsx::Lock::acquire(&path)?;
    let summed = read_summed(store, agent)?;
    if summed.overflow_line.is_none() && !summed.torn {
        let _ = ledger_meta::store_entries(&path, &summed.entries);
    }
    Ok(summed)
}

pub fn read(store: &Store, agent: &str) -> Result<Read, String> {
    let path = store.ledger_path(agent)?;
    read_at(&path, agent, true)
}

/// Read a ledger file. `enforce_cap` is false for an archived file, which is
/// allowed to be larger than the live ledger.
pub fn read_at(path: &std::path::Path, agent: &str, enforce_cap: bool) -> Result<Read, String> {
    let text = match fsx::file_kind(path)? {
        fsx::FileKind::Missing => {
            return Ok(Read {
                entries: Vec::new(),
                lines: Vec::new(),
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
        fsx::FileKind::Regular { len } if enforce_cap && len > MAX_LEDGER_BYTES => {
            return Err(format!(
                "ledger {} is {len} bytes, over the {MAX_LEDGER_BYTES} byte cap",
                path.display()
            ))
        }
        fsx::FileKind::Regular { .. } => fsx::read_strict(path)?,
    };
    parse_text(path, agent, &text)
}

fn parse_text(path: &std::path::Path, agent: &str, text: &str) -> Result<Read, String> {
    let torn = !text.is_empty() && !text.ends_with('\n');
    let mut lines: Vec<&str> = text.lines().collect();
    if torn {
        lines.pop();
    }
    let mut entries = Vec::new();
    let mut numbers = Vec::new();
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
        numbers.push(i + 1);
    }
    Ok(Read {
        entries,
        lines: numbers,
        warnings,
        torn,
    })
}

pub fn total(entries: &[Entry]) -> Result<Totals, String> {
    let (totals, bad) = sum_prefix(entries);
    match bad {
        Some(index) => Err(format!(
            "ledger totals overflowed a 64-bit number at entry {}",
            index + 1
        )),
        None => Ok(totals),
    }
}

/// Totals of the leading entries that fit in 64 bits, and the 0-based index
/// of the first entry that does not fit. This is an index into `entries`,
/// not a file line: use `Read::sum` to name a line.
pub fn sum_prefix(entries: &[Entry]) -> (Totals, Option<usize>) {
    let mut totals = Totals::default();
    for (i, entry) in entries.iter().enumerate() {
        if totals.add(entry).is_err() {
            return (totals, Some(i));
        }
    }
    (totals, None)
}

/// Totals grouped by task, sorted by task name.
pub fn by_task(entries: &[Entry]) -> Result<Vec<(String, Totals)>, String> {
    Ok(finish_groups(grouped(entries, |e| e.task.clone())?, false))
}

/// Task totals with the most recently written task first.
pub fn by_task_recent(entries: &[Entry]) -> Result<Vec<(String, Totals)>, String> {
    Ok(finish_groups(grouped(entries, |e| e.task.clone())?, true))
}

/// Totals grouped by step, sorted by step name. A missing step is `""`.
pub fn by_step(entries: &[Entry]) -> Result<Vec<(String, Totals)>, String> {
    Ok(finish_groups(
        grouped(entries, |e| e.step.clone().unwrap_or_default())?,
        false,
    ))
}

struct Group {
    key: String,
    totals: Totals,
    last: usize,
}

fn grouped(entries: &[Entry], key_of: impl Fn(&Entry) -> String) -> Result<Vec<Group>, String> {
    let mut map: HashMap<String, Group> = HashMap::new();
    for (i, entry) in entries.iter().enumerate() {
        let key = key_of(entry);
        if let Some(group) = map.get_mut(&key) {
            group.totals.add(entry)?;
            group.last = i;
        } else {
            let mut totals = Totals::default();
            totals.add(entry)?;
            map.insert(
                key.clone(),
                Group {
                    key,
                    totals,
                    last: i,
                },
            );
        }
    }
    Ok(map.into_values().collect())
}

fn finish_groups(mut rows: Vec<Group>, recent: bool) -> Vec<(String, Totals)> {
    if recent {
        rows.sort_by(|a, b| b.last.cmp(&a.last).then_with(|| a.key.cmp(&b.key)));
    } else {
        rows.sort_by(|a, b| a.key.cmp(&b.key));
    }
    rows.into_iter().map(|row| (row.key, row.totals)).collect()
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
