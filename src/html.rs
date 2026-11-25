//! `.libellus/board.html`: a read-only page of the board for people.
//!
//! One self-contained file: inline CSS, no scripts, no fonts or images
//! loaded from anywhere, so it works offline and from `file://`. Collapsing
//! uses `<details>`, which needs no JavaScript. Light and dark follow
//! `prefers-color-scheme`. Every value from the store is escaped: titles,
//! outcomes and summaries are agent input.

use crate::board::{self, Board, Task, COLUMNS};
use crate::fsx;
use crate::note;
use crate::store::Store;
use std::path::PathBuf;

/// Escape for HTML text and for a quoted attribute value.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Open questions of one agent, for the page.
pub struct Questions {
    pub agent: String,
    pub items: Vec<(String, String)>,
    pub error: Option<String>,
}

pub fn questions(store: &Store) -> Vec<Questions> {
    let agents = match store.agents() {
        Ok(a) => a,
        Err(e) => {
            return vec![Questions {
                agent: "(store)".into(),
                items: Vec::new(),
                error: Some(e),
            }]
        }
    };
    agents
        .into_iter()
        .filter_map(|agent| match note::load_all(store, &agent) {
            Ok(loaded) => {
                let items: Vec<(String, String)> = loaded
                    .notes
                    .iter()
                    .rev()
                    .filter(|n| n.kind == "question" && n.status == "open")
                    .map(|n| (n.id.clone(), n.summary.clone()))
                    .collect();
                (!items.is_empty()).then_some(Questions {
                    agent,
                    items,
                    error: None,
                })
            }
            Err(e) => Some(Questions {
                agent,
                items: Vec::new(),
                error: Some(e),
            }),
        })
        .collect()
}

/// Write the page to `.libellus/board.html`. Called after every board write,
/// under the board lock, so a later write's page is never replaced by an
/// earlier one's.
pub fn write_page(store: &Store, live: &Board) -> Result<PathBuf, String> {
    let archived = board::load_archive(store)?;
    let page = render(store, live, &archived, &questions(store), &fsx::now_iso());
    let path = fsx::join_child(&store.dir, &["board.html"])?;
    fsx::refuse_special(&path)?;
    fsx::write_atomic(&path, page.as_bytes(), true)?;
    Ok(path)
}

/// Load everything and write the page to `out`, or to `.libellus/board.html`.
pub fn regenerate(store: &Store, out: Option<&str>) -> Result<PathBuf, String> {
    let live = board::load(store)?;
    let archived = board::load_archive(store)?;
    let page = render(store, &live, &archived, &questions(store), &fsx::now_iso());
    let path = match out {
        Some(p) => PathBuf::from(p),
        None => fsx::join_child(&store.dir, &["board.html"])?,
    };
    fsx::refuse_special(&path)?;
    fsx::write_atomic(&path, page.as_bytes(), true)?;
    Ok(path)
}

const STYLE: &str = r#"
:root{--bg:#fbfbf8;--fg:#1d1d1b;--muted:#66645e;--card:#fff;--line:#deddd6;--accent:#2f6f4f;--warn:#9a5b00;--bad:#a23b2a;--banner:#fff4d6}
@media (prefers-color-scheme:dark){:root{--bg:#161614;--fg:#ecebe4;--muted:#a3a198;--card:#21211e;--line:#3a3934;--accent:#7cc39b;--warn:#e0a84a;--bad:#e78b78;--banner:#3a3218}}
*{box-sizing:border-box}
body{margin:0;padding:16px;background:var(--bg);color:var(--fg);font:15px/1.45 system-ui,-apple-system,"Segoe UI",sans-serif;max-width:1200px;margin-inline:auto}
h1{font-size:1.4rem;margin:.2rem 0 .6rem}h2{font-size:1.1rem;margin:1.6rem 0 .6rem;border-bottom:1px solid var(--line);padding-bottom:.2rem}
.banner{background:var(--banner);border:1px solid var(--line);border-radius:8px;padding:.6rem .8rem}
.muted{color:var(--muted)}code{font:13px ui-monospace,Menlo,Consolas,monospace;overflow-wrap:anywhere}
.cols{display:grid;grid-template-columns:repeat(auto-fit,minmax(15rem,1fr));gap:12px}
.col{background:transparent;border:1px solid var(--line);border-radius:8px;padding:8px;min-width:0}
.col h3{margin:.1rem 0 .5rem;font-size:.95rem;text-transform:uppercase;letter-spacing:.04em}
.card{background:var(--card);border:1px solid var(--line);border-radius:6px;padding:6px 8px;margin:0 0 8px;overflow-wrap:anywhere}
.card .t{font-weight:600}.meta{font-size:12px;color:var(--muted)}
.s-doing{border-left:4px solid var(--accent)}.s-blocked{border-left:4px solid var(--warn)}.s-done{opacity:.8}.s-dropped{opacity:.6;text-decoration:line-through}
ul.tree{list-style:none;padding-left:1.1rem;margin:.2rem 0}ul.tree.top{padding-left:0}
ul.tree li{margin:.15rem 0;overflow-wrap:anywhere}
input[type=checkbox]{margin-right:.35rem;vertical-align:-2px}
progress{width:10rem;max-width:40vw;vertical-align:middle}
details{margin:.4rem 0}summary{cursor:pointer}
table{border-collapse:collapse}td,th{padding:2px 8px 2px 0;text-align:left;vertical-align:top}
footer{margin-top:2rem;font-size:12px;color:var(--muted);overflow-wrap:anywhere}
"#;

pub fn render(
    store: &Store,
    live: &Board,
    archived: &[board::Archived],
    questions: &[Questions],
    now: &str,
) -> String {
    let mut h = String::with_capacity(4096 + live.tasks.len() * 600);
    h.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<meta name=\"color-scheme\" content=\"light dark\">\n<title>Libellus board</title>\n<style>");
    h.push_str(STYLE);
    h.push_str("</style>\n</head>\n<body>\n<h1>Libellus board</h1>\n");
    h.push_str("<p class=\"banner\"><strong>Read-only.</strong> Agents keep this board; this page cannot change it. To change anything, ask your agent, for example: <code>review task &lt;id&gt;</code> or <code>drop task &lt;id&gt;</code>.</p>\n");
    let c = live.counts();
    h.push_str(&format!(
        "<p class=\"muted\">{} open tasks: {} todo, {} doing, {} blocked. {} done, {} dropped. {} archived projects.</p>\n",
        c.todo + c.doing + c.blocked,
        c.todo,
        c.doing,
        c.blocked,
        c.done,
        c.dropped,
        archived.len()
    ));
    for w in &live.warnings {
        h.push_str(&format!("<p class=\"banner\">Warning: {}</p>\n", esc(w)));
    }

    // Projects and their progress.
    h.push_str("<h2>Projects</h2>\n");
    if live.roots().is_empty() {
        h.push_str("<p class=\"muted\">No open projects.</p>\n");
    } else {
        h.push_str("<table>\n");
        for root in live.roots() {
            let Some(t) = live.get(root) else {
                continue;
            };
            let (done, total) = progress(live, root);
            h.push_str(&format!(
                "<tr><td><code>{}</code></td><td>{}</td><td><progress max=\"{total}\" value=\"{done}\">{done}/{total}</progress></td><td>{done}/{total} done</td></tr>\n",
                esc(&t.id),
                esc(&t.title)
            ));
        }
        h.push_str("</table>\n");
    }

    // Kanban.
    h.push_str("<h2>Kanban</h2>\n<div class=\"cols\">\n");
    let order = live.tree_order();
    for col in COLUMNS {
        let cards: Vec<&Task> = order
            .iter()
            .map(|(t, _)| *t)
            .filter(|t| t.status == *col)
            .collect();
        h.push_str(&format!(
            "<section class=\"col\"><h3>{col} ({})</h3>\n",
            cards.len()
        ));
        for t in cards {
            card(&mut h, live, t);
        }
        h.push_str("</section>\n");
    }
    h.push_str("</div>\n");
    let dropped: Vec<&Task> = order
        .iter()
        .map(|(t, _)| *t)
        .filter(|t| t.status == "dropped")
        .collect();
    if !dropped.is_empty() {
        h.push_str(&format!(
            "<details><summary>Dropped ({})</summary>\n",
            dropped.len()
        ));
        for t in dropped {
            card(&mut h, live, t);
        }
        h.push_str("</details>\n");
    }

    // Tree.
    h.push_str("<h2>Tree</h2>\n");
    if live.roots().is_empty() {
        h.push_str("<p class=\"muted\">The board is empty.</p>\n");
    }
    for root in live.roots() {
        tree(&mut h, live, root);
    }

    // Open questions, from each agent's notes.
    h.push_str("<h2>Open questions</h2>\n");
    if questions.is_empty() {
        h.push_str("<p class=\"muted\">No agent has an open question.</p>\n");
    }
    for q in questions {
        h.push_str(&format!("<h3>{}</h3>\n", esc(&q.agent)));
        if let Some(e) = &q.error {
            h.push_str(&format!(
                "<p class=\"banner\">Notes could not be read: {}</p>\n",
                esc(e)
            ));
        }
        if !q.items.is_empty() {
            h.push_str("<ul>\n");
            for (id, summary) in &q.items {
                h.push_str(&format!(
                    "<li><code>{}</code> {}</li>\n",
                    esc(id),
                    esc(summary)
                ));
            }
            h.push_str("</ul>\n");
        }
    }

    // Archive, newest last, collapsed.
    if !archived.is_empty() {
        h.push_str("<h2>Archived projects</h2>\n");
        for a in archived {
            let root = a.board.roots().first().cloned().unwrap_or_default();
            let title = a.board.get(&root).map(|t| t.title.as_str()).unwrap_or("");
            let closed = a
                .board
                .get(&root)
                .and_then(|t| t.closed.as_deref())
                .unwrap_or("");
            let (done, total) = progress(&a.board, &root);
            h.push_str(&format!(
                "<details><summary><code>{}</code> {} <span class=\"muted\">closed {} ({done}/{total} done)</span></summary>\n",
                esc(&a.name),
                esc(title),
                esc(closed)
            ));
            for r in a.board.roots() {
                tree(&mut h, &a.board, r);
            }
            h.push_str("</details>\n");
        }
    }

    h.push_str(&format!(
        "<footer>Generated {} by libellus {} from <code>{}</code>. Regenerated after every board change, or with <code>libellus board --html</code>.</footer>\n</body>\n</html>\n",
        esc(now),
        crate::VERSION,
        esc(&store.dir.display().to_string())
    ));
    h
}

/// Done and total tasks under and including `root`.
fn progress(b: &Board, root: &str) -> (usize, usize) {
    let mut ids = vec![root.to_string()];
    ids.extend(b.descendants(root));
    let done = ids
        .iter()
        .filter(|i| b.get(i).is_some_and(|t| t.status == "done"))
        .count();
    (done, ids.len())
}

fn details(h: &mut String, t: &Task) {
    h.push_str(&format!(
        "<div class=\"meta\"><code>{}</code> &middot; {} &middot; {} &middot; updated {} by {}</div>\n",
        esc(&t.id),
        esc(&t.owner),
        esc(&t.status),
        esc(&t.updated),
        esc(&t.updated_by)
    ));
    if let Some(o) = &t.outcome {
        h.push_str(&format!("<div class=\"meta\">Outcome: {}</div>\n", esc(o)));
    }
    if !t.evidence.is_empty() {
        let list: Vec<String> = t
            .evidence
            .iter()
            .map(|e| format!("<code>{}</code>", esc(e)))
            .collect();
        h.push_str(&format!(
            "<div class=\"meta\">Evidence: {}</div>\n",
            list.join(", ")
        ));
    }
}

fn card(h: &mut String, b: &Board, t: &Task) {
    let path = b.path(&t.id);
    h.push_str(&format!(
        "<div class=\"card s-{}\"><div class=\"t\">{}</div>\n",
        esc(&t.status),
        esc(&t.title)
    ));
    if path.len() > 1 {
        h.push_str(&format!(
            "<div class=\"meta\">{}</div>\n",
            esc(&path[..path.len() - 1].join(" > "))
        ));
    }
    details(h, t);
    h.push_str("</div>\n");
}

fn tree(h: &mut String, b: &Board, root: &str) {
    let Some(t) = b.get(root) else {
        return;
    };
    let (done, total) = progress(b, root);
    h.push_str(&format!(
        "<details open><summary><strong>{}</strong> <span class=\"muted\">{done}/{total} done</span></summary>\n<ul class=\"tree top\">\n",
        esc(&t.title)
    ));
    node(h, b, t);
    h.push_str("</ul>\n</details>\n");
}

fn node(h: &mut String, b: &Board, t: &Task) {
    let checked = if t.status == "done" { " checked" } else { "" };
    h.push_str(&format!(
        "<li class=\"s-{}\"><label><input type=\"checkbox\" disabled{checked} aria-label=\"{}\">{}</label> <span class=\"meta\">[{}]</span>\n",
        esc(&t.status),
        esc(&t.status),
        esc(&t.title),
        esc(&t.status)
    ));
    details(h, t);
    let kids = b.children(&t.id);
    if !kids.is_empty() {
        h.push_str("<ul class=\"tree\">\n");
        for k in kids {
            if let Some(kt) = b.get(k) {
                node(h, b, kt);
            }
        }
        h.push_str("</ul>\n");
    }
    h.push_str("</li>\n");
}
