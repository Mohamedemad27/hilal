//! What Claude Code is doing: reads the session transcripts Claude Code keeps under
//! ~/.claude/projects (one JSON-lines file per session) and sums up the recent ones: their
//! title and folder, whether Claude is working, waiting on you, done or stopped midway,
//! the task list it's keeping, and the latest exchange. Only the end of each file is read
//! (they grow to hundreds of MB), and only again when it changes.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use serde_json::Value;

/// How much of the end of each transcript to read.
const TAIL: u64 = 3 << 20;
/// Sessions touched within this long show up.
const RECENT: Duration = Duration::from_secs(3 * 24 * 3600);
/// Mid-turn with no writes for this long counts as stopped.
const STALLED: Duration = Duration::from_secs(10 * 60);
const MAX_SESSIONS: usize = 8;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum State {
    /// Mid-turn: thinking or running tools.
    Working,
    /// Asked you something and is waiting for the answer.
    NeedsYou,
    /// Finished its turn.
    Done,
    /// Was mid-turn but nothing has happened for a while (interrupted or stuck).
    Stopped,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Task {
    Pending,
    Active,
    Done,
}

#[derive(Clone, Debug)]
pub struct Session {
    pub id: String,
    pub title: String,
    /// The folder it works in (last part of the path).
    pub project: String,
    pub modified: SystemTime,
    pub state: State,
    /// What it's doing right now, while working (a tool's own description, or its name).
    pub doing: String,
    pub tasks: Vec<(String, Task)>,
    /// Your latest message.
    pub prompt: String,
    /// Claude's latest words: its turn summary if it gave one, else its last text.
    pub reply: String,
}

impl Session {
    pub fn tasks_done(&self) -> (usize, usize) {
        (self.tasks.iter().filter(|t| t.1 == Task::Done).count(), self.tasks.len())
    }

    pub fn age(&self) -> Duration {
        SystemTime::now().duration_since(self.modified).unwrap_or_default()
    }
}

/// Keeps a fresh list of recent sessions, newest first, from a background thread.
pub struct Sessions {
    list: Arc<Mutex<Vec<Session>>>,
}

impl Sessions {
    pub fn start() -> Self {
        let list = Arc::new(Mutex::new(Vec::new()));
        let shared = list.clone();
        std::thread::spawn(move || {
            let mut cache: HashMap<PathBuf, (u64, SystemTime, Session)> = HashMap::new();
            loop {
                let found = scan(&mut cache);
                *shared.lock().unwrap() = found;
                std::thread::sleep(Duration::from_millis(1500));
            }
        });
        Self { list }
    }

    pub fn list(&self) -> Vec<Session> {
        self.list.lock().unwrap().clone()
    }
}

fn root() -> PathBuf {
    std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default().join(".claude").join("projects")
}

fn scan(cache: &mut HashMap<PathBuf, (u64, SystemTime, Session)>) -> Vec<Session> {
    let mut files: Vec<(PathBuf, u64, SystemTime)> = Vec::new();
    for dir in std::fs::read_dir(root()).into_iter().flatten().flatten() {
        for f in std::fs::read_dir(dir.path()).into_iter().flatten().flatten() {
            let p = f.path();
            if p.extension().is_some_and(|e| e == "jsonl") {
                if let Ok(m) = f.metadata() {
                    let t = m.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                    if m.len() > 0 && SystemTime::now().duration_since(t).unwrap_or_default() < RECENT {
                        files.push((p, m.len(), t));
                    }
                }
            }
        }
    }
    files.sort_by(|a, b| b.2.cmp(&a.2));
    files.truncate(MAX_SESSIONS);
    let mut out = Vec::new();
    for (p, len, t) in files {
        let s = match cache.get(&p) {
            Some((l, m, s)) if *l == len && *m == t => Some(s.clone()),
            _ => read_session(&p, t),
        };
        if let Some(mut s) = s {
            // Time alone can turn working into stopped.
            if s.state == State::Working && s.age() > STALLED {
                s.state = State::Stopped;
            }
            cache.insert(p, (len, t, s.clone()));
            out.push(s);
        }
    }
    out
}

fn read_session(path: &Path, modified: SystemTime) -> Option<Session> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let start = len.saturating_sub(TAIL);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    let mut lines = text.lines();
    if start > 0 {
        lines.next(); // most likely cut in half
    }
    let entries: Vec<Value> = lines.filter_map(|l| serde_json::from_str(l).ok()).collect();
    let id = path.file_stem()?.to_string_lossy().into_owned();
    let mut s = summarize(&id, &entries);
    s.modified = modified;
    Some(s)
}

/// Text blocks of a message's content, joined.
fn text_of(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Squeezes whitespace and drops the system notes the app wraps around messages.
fn clean(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    // Drop <system-reminder>…</system-reminder> and similar tagged blocks.
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        let tag_end = rest[i..].find('>').map(|j| i + j);
        let Some(te) = tag_end else { break };
        let tag = &rest[i + 1..te];
        if tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') && !tag.is_empty() {
            let close = format!("</{tag}>");
            if let Some(ce) = rest[te..].find(&close) {
                rest = &rest[te + ce + close.len()..];
                continue;
            }
        }
        out.push('<');
        rest = &rest[i + 1..];
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn summarize(id: &str, entries: &[Value]) -> Session {
    let mut s = Session {
        id: id.to_owned(),
        title: String::new(),
        project: String::new(),
        modified: SystemTime::UNIX_EPOCH,
        state: State::Done,
        doing: String::new(),
        tasks: Vec::new(),
        prompt: String::new(),
        reply: String::new(),
    };
    let mut recap = String::new();
    let mut last_text = String::new();
    // The conversation's last step: (is it Claude's, did its turn end, open tool name).
    let mut last: Option<(bool, bool, Option<(String, String)>)> = None;

    for e in entries {
        if e["isSidechain"] == true {
            continue;
        }
        if let Some(cwd) = e["cwd"].as_str() {
            s.project = Path::new(cwd).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        }
        match e["type"].as_str() {
            Some("custom-title") => s.title = e["customTitle"].as_str().unwrap_or_default().to_owned(),
            Some("last-prompt") => s.prompt = clean(e["lastPrompt"].as_str().unwrap_or_default()),
            Some("assistant") => {
                let m = &e["message"];
                let ended = m["stop_reason"] == "end_turn";
                let mut open_tool = None;
                for b in m["content"].as_array().into_iter().flatten() {
                    match b["type"].as_str() {
                        Some("text") => {
                            let t = clean(b["text"].as_str().unwrap_or_default());
                            if !t.is_empty() {
                                last_text = t;
                            }
                        }
                        Some("tool_use") => {
                            let name = b["name"].as_str().unwrap_or_default().to_owned();
                            let input = &b["input"];
                            plan_step(&mut s.tasks, &name, input, &mut recap);
                            let what = input["description"].as_str().map(clean).unwrap_or_default();
                            open_tool = Some((name, what));
                        }
                        _ => {}
                    }
                }
                last = Some((true, ended, open_tool));
            }
            Some("user") => {
                let content = &e["message"]["content"];
                let is_result = content.as_array().is_some_and(|a| a.iter().any(|b| b["type"] == "tool_result"));
                if !is_result {
                    let t = clean(&text_of(content));
                    if !t.is_empty() {
                        s.prompt = t;
                    }
                }
                last = Some((false, false, None));
            }
            _ => {}
        }
    }
    s.reply = if recap.is_empty() { last_text } else { recap };
    s.state = match &last {
        Some((true, true, _)) => State::Done,
        Some((true, false, Some((tool, _)))) if tool == "AskUserQuestion" => State::NeedsYou,
        Some((true, false, Some((tool, _)))) if tool.ends_with("wrap_up") => State::Done,
        None => State::Done,
        _ => State::Working,
    };
    if let Some((_, _, Some((tool, what)))) = &last {
        s.doing = if what.is_empty() { friendly_tool(tool) } else { what.clone() };
    }
    if s.title.is_empty() {
        s.title = s.prompt.chars().take(60).collect();
    }
    s
}

/// A tool's name as words ("mcp__x__update_plan" → "Update plan").
fn friendly_tool(name: &str) -> String {
    let base = name.rsplit("__").next().unwrap_or(name).replace('_', " ");
    let mut c = base.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// Follows the task-list tools: the app's plan list (whole list, advance, done/start by
/// number), the CLI's TodoWrite, and the wrap-up that closes a turn.
fn plan_step(tasks: &mut Vec<(String, Task)>, name: &str, input: &Value, recap: &mut String) {
    if name.ends_with("update_plan") {
        if let Some(items) = input["items"].as_array() {
            *tasks = items
                .iter()
                .map(|i| (i["content"].as_str().unwrap_or_default().to_owned(), task_state(&i["status"])))
                .collect();
            recap.clear();
        }
        if input["advance"] == true {
            if let Some(i) = tasks.iter().position(|t| t.1 == Task::Active) {
                tasks[i].1 = Task::Done;
            }
            if let Some(i) = tasks.iter().position(|t| t.1 == Task::Pending) {
                tasks[i].1 = Task::Active;
            }
        }
        for n in input["done"].as_array().into_iter().flatten().filter_map(|v| v.as_u64()) {
            if let Some(t) = tasks.get_mut(n as usize - 1) {
                t.1 = Task::Done;
            }
        }
        if let Some(n) = input["start"].as_u64() {
            for t in tasks.iter_mut().filter(|t| t.1 == Task::Active) {
                t.1 = Task::Done;
            }
            if let Some(t) = tasks.get_mut(n as usize - 1) {
                t.1 = Task::Active;
            }
        }
    } else if name == "TodoWrite" {
        if let Some(items) = input["todos"].as_array() {
            *tasks = items
                .iter()
                .map(|i| (i["content"].as_str().unwrap_or_default().to_owned(), task_state(&i["status"])))
                .collect();
        }
    } else if name.ends_with("wrap_up") {
        for t in tasks.iter_mut() {
            t.1 = Task::Done;
        }
        *recap = clean(input["recap"].as_str().unwrap_or_default());
    }
}

fn task_state(v: &Value) -> Task {
    match v.as_str() {
        Some("completed") => Task::Done,
        Some("in_progress") => Task::Active,
        _ => Task::Pending,
    }
}

/// "now", "5m", "3h", "2d".
pub fn ago(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0..=59 => "now".to_owned(),
        60..=3599 => format!("{}m", s / 60),
        3600..=86399 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tool(name: &str, input: Value) -> Value {
        json!({"type": "assistant", "cwd": "K:\\hilal", "message": {"stop_reason": "tool_use",
            "content": [{"type": "tool_use", "name": name, "input": input}]}})
    }

    #[test]
    fn follows_the_plan_and_the_turn() {
        let e = vec![
            json!({"type": "custom-title", "customTitle": "Build the page"}),
            json!({"type": "user", "message": {"content": "<system-reminder>x</system-reminder>make   it  modern"}}),
            tool("mcp__ccd_turn__update_plan", json!({"items": [
                {"content": "Read", "status": "in_progress"}, {"content": "Write", "status": "pending"}, {"content": "Test", "status": "pending"}]})),
            json!({"type": "user", "message": {"content": [{"type": "tool_result"}]}}),
            tool("mcp__ccd_turn__update_plan", json!({"advance": true})),
            tool("Bash", json!({"description": "Build Hilal"})),
        ];
        let s = summarize("id", &e);
        assert_eq!(s.title, "Build the page");
        assert_eq!(s.project, "hilal");
        assert_eq!(s.prompt, "make it modern");
        assert_eq!(s.state, State::Working);
        assert_eq!(s.doing, "Build Hilal");
        assert_eq!(s.tasks.iter().map(|t| t.1).collect::<Vec<_>>(), [Task::Done, Task::Active, Task::Pending]);

        let mut e2 = e.clone();
        e2.push(tool("mcp__ccd_turn__wrap_up", json!({"recap": "All built."})));
        let s = summarize("id", &e2);
        assert_eq!(s.state, State::Done);
        assert_eq!(s.reply, "All built.");
        assert_eq!(s.tasks_done(), (3, 3));

        let mut e3 = e.clone();
        e3.push(tool("AskUserQuestion", json!({})));
        assert_eq!(summarize("id", &e3).state, State::NeedsYou);

        let mut e4 = e.clone();
        e4.push(json!({"type": "assistant", "message": {"stop_reason": "end_turn", "content": [{"type": "text", "text": "Done!"}]}}));
        let s = summarize("id", &e4);
        assert_eq!((s.state, s.reply.as_str()), (State::Done, "Done!"));
    }

    #[test]
    fn ages_read_short() {
        assert_eq!(ago(Duration::from_secs(30)), "now");
        assert_eq!(ago(Duration::from_secs(300)), "5m");
        assert_eq!(ago(Duration::from_secs(7200)), "2h");
        assert_eq!(ago(Duration::from_secs(200000)), "2d");
    }
}

#[cfg(test)]
mod live {
    /// Prints what it makes of this PC's recent sessions (run with --ignored --nocapture).
    #[test]
    #[ignore]
    fn prints_recent_sessions() {
        for s in super::scan(&mut Default::default()) {
            let (d, n) = s.tasks_done();
            println!("{:?} | {} | {} | {} | tasks {d}/{n} | doing: {}", s.state, super::ago(s.age()), s.project, s.title, s.doing);
            for t in &s.tasks {
                println!("     {:?} {}", t.1, t.0);
            }
            println!("   you: {}", s.prompt.chars().take(100).collect::<String>());
            println!("   claude: {}", s.reply.chars().take(140).collect::<String>());
        }
    }
}
