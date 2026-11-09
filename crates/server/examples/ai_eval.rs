//! Tries the instructions for AI organizing, or for tidying a folder, on a set of cases with a real
//! model, and checks each answer against what its case expects. The model and its key are given in
//! the environment; how, and what is checked, in docs/ai.md#评测.
//!
//! The answers go to target/ai-eval, for reading them through.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::Parser;
use nexttodo_core::board::{Board, Snapshot};
use nexttodo_core::dag::find_path;
use nexttodo_core::model::{AiJob, Due, RepeatFrom, RepeatUnit, Suggestion, TaskId};
use nexttodo_core::ops::{MAX_OPS, Op};
use nexttodo_server::ai::llm::{Api, Config, Https, Prompt, Thinking};
use nexttodo_server::ai::{organize, tidy};
use nexttodo_server::tools;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Parser)]
struct Args {
    /// Files of cases, each a JSON array.
    #[arg(required = true)]
    cases: Vec<PathBuf>,
    /// Instructions to try in place of the server's.
    #[arg(long)]
    system: Option<PathBuf>,
    /// Asks each case this many times: a model answers differently each time.
    #[arg(long, default_value_t = 1)]
    repeat: usize,
    /// How many to ask at once.
    #[arg(long, default_value_t = 4)]
    jobs: usize,
    /// Only the cases whose id has this in it.
    #[arg(long)]
    only: Option<String>,
    #[arg(long, env = "NEXTTODO_EVAL_URL")]
    url: String,
    #[arg(long, env = "NEXTTODO_EVAL_MODEL")]
    model: String,
    /// One of the providers in llm.rs, for how to set the thinking level and JSON mode.
    #[arg(long, env = "NEXTTODO_EVAL_PROVIDER", default_value = "custom")]
    provider: String,
    #[arg(long, env = "NEXTTODO_EVAL_THINKING", default_value = "off")]
    thinking: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCase {
    id: String,
    /// What it tries, for people.
    #[serde(default)]
    #[allow(dead_code)]
    note: String,
    /// What the server gives the model: for organizing, local_time, folder, open_tasks and text; for
    /// tidying, what `tidy::prompt` sends, with `request`.
    input: Value,
    expect: Value,
}

struct Case {
    id: String,
    input: Value,
    expect: Kind,
}

enum Kind {
    Organize(Expect),
    /// The folder the input shows, and the job tidying it.
    Tidy(TidyExpect, Box<(Snapshot, AiJob)>),
}

/// What a good answer does. Whatever is left out is not checked.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Expect {
    /// How many tasks: at least, at most.
    tasks: Option<(usize, usize)>,
    /// Each in some title or description, in one of its wordings.
    #[serde(default)]
    facts: Vec<Vec<String>>,
    /// Each in some title or description, exactly as written: prompts, code, links.
    #[serde(default)]
    verbatim: Vec<String>,
    /// Nowhere: what the text took back, say.
    #[serde(default)]
    absent: Vec<String>,
    /// Whether descriptions may be written: not when the title says all there is.
    description: Option<bool>,
    /// The text is written prose already: its words may stay.
    #[serde(default)]
    written: bool,
    /// The due times, in any order.
    due: Option<Vec<String>>,
    remind: Option<Vec<u32>>,
    /// The open tasks made to come before the new ones, and after them.
    after: Option<Vec<i64>>,
    before: Option<Vec<i64>>,
    /// Whether the new tasks are linked to each other.
    linked: Option<bool>,
    /// Words in the titles of new tasks, in the order the tasks are to be done.
    #[serde(default)]
    order: Vec<String>,
}

/// Spoken words, out of place in a description.
const SPOKEN: &[&str] = &[
    "我觉得",
    "我感觉",
    "我想",
    "我建议",
    "我希望",
    "我发现",
    "就是说",
    "然后就是",
    "然后呢",
    "那个",
    "嗯",
    "呃",
    "好像",
    "其实",
    "反正",
    "或者说",
    "比如说",
    "帮我记",
];
/// Runs of this many Chinese characters shared with the text count as copied. Only these: names,
/// terms and links in other scripts stay as they are anyway.
const RUN: usize = 6;
/// Particles that end spoken sentences, when they do: before a stop or at the end.
const PARTICLES: &[&str] = &["吧", "呢", "啊", "嘛"];
/// A description made mostly of runs of the text is the text, not a rewrite of it.
const COPIED: f64 = 0.5;
/// Nor is a description made of the title's words more than the title.
const REPEATED: f64 = 0.7;
const TIMEOUT: Duration = Duration::from_secs(300);

/// What a good tidying suggests. Whatever is left out is not checked.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TidyExpect {
    /// What may be done at all, of `title`, `description`, `due`, `done`, `reopen`, `folder`,
    /// `after`, `unlink`, `delete`, `create` and `order`.
    allowed: Option<Vec<String>>,
    /// The tasks deleted, done and opened again: exactly these.
    deleted: Option<Vec<i64>>,
    /// How many are deleted, whichever they are.
    deletes: Option<usize>,
    done: Option<Vec<i64>>,
    reopened: Option<Vec<i64>>,
    /// Tasks moved, and where to.
    moved: Option<Vec<(i64, String)>>,
    /// The due dates changed, exactly these, as `due_text` writes them; empty for one cleared.
    due: Option<Vec<(i64, String)>>,
    /// New tasks' due dates, a word in the title for each.
    #[serde(default)]
    new_due: Vec<(String, String)>,
    /// Tasks before others once everything is taken, through others maybe, and not; and tasks no
    /// longer right before others. A task is given by its number, or by a word in its title then.
    #[serde(default)]
    linked: Vec<(Who, Who)>,
    #[serde(default)]
    not_linked: Vec<(Who, Who)>,
    #[serde(default)]
    unlinked: Vec<(i64, i64)>,
    /// How many new tasks: at least, at most.
    created: Option<(usize, usize)>,
    /// Tasks nothing is suggested for.
    #[serde(default)]
    untouched: Vec<i64>,
    /// Each in some title or description once everything is taken, in one of its wordings.
    #[serde(default)]
    facts: Vec<Vec<String>>,
    #[serde(default)]
    verbatim: Vec<String>,
    #[serde(default)]
    absent: Vec<String>,
    /// Tasks in the order a reordering is to put them.
    #[serde(default)]
    order: Vec<i64>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Who {
    Number(i64),
    Words(String),
}

struct Outcome {
    case: usize,
    run: usize,
    answer: String,
    problems: Vec<String>,
    facts: (usize, usize),
    seconds: f64,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let key = std::env::var("NEXTTODO_EVAL_KEY").map_err(|_| anyhow::anyhow!("缺少环境变量 NEXTTODO_EVAL_KEY"))?;
    let thinking = Thinking::parse(&args.thinking).ok_or_else(|| anyhow::anyhow!("思考程度是 off、low 或 high"))?;
    let config = Config { provider: args.provider, base_url: args.url, model: args.model, thinking, api_key: key };
    config.check().map_err(anyhow::Error::msg)?;
    let system = args.system.as_ref().map(std::fs::read_to_string).transpose()?;
    let mut cases = Vec::new();
    for path in &args.cases {
        let read: Vec<RawCase> =
            serde_json::from_str(&std::fs::read_to_string(path)?).map_err(|e| anyhow::anyhow!("{}：{e}", path.display()))?;
        for c in read {
            let expect = if c.input.get("request").is_some() {
                let data = tidy_data(&c.input);
                // So that the case is what the server would send.
                let sent: Value = serde_json::from_str(&tidy::prompt(&data.1, data.0.clone()).0.user)?;
                anyhow::ensure!(sent == c.input, "{}：服务端发给模型的会是\n{}", c.id, serde_json::to_string_pretty(&sent)?);
                Kind::Tidy(serde_json::from_value(c.expect).map_err(|e| anyhow::anyhow!("{}：{e}", c.id))?, Box::new(data))
            } else {
                Kind::Organize(serde_json::from_value(c.expect).map_err(|e| anyhow::anyhow!("{}：{e}", c.id))?)
            };
            cases.push(Case { id: c.id, input: c.input, expect });
        }
    }
    cases.retain(|c| args.only.as_ref().is_none_or(|only| c.id.contains(only.as_str())));

    let work: Vec<(usize, usize)> = (0..args.repeat).flat_map(|run| (0..cases.len()).map(move |case| (case, run))).collect();
    let next = AtomicUsize::new(0);
    let outcomes = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..args.jobs.max(1) {
            s.spawn(|| {
                while let Some(&(case, run)) = work.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let outcome = ask(&config, system.as_deref(), &cases[case], case, run);
                    outcomes.lock().unwrap().push(outcome);
                }
            });
        }
    });
    let mut outcomes = outcomes.into_inner().unwrap();
    outcomes.sort_by_key(|o| (o.case, o.run));

    let label = args.system.as_ref().and_then(|p| p.file_stem()).map_or("current".into(), |s| s.to_string_lossy().into_owned());
    let secs = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let dir = PathBuf::from("target/ai-eval").join(format!("{secs}-{label}"));
    std::fs::create_dir_all(&dir)?;
    let saved: Vec<Value> = outcomes
        .iter()
        .map(|o| json!({ "id": cases[o.case].id, "run": o.run, "problems": o.problems, "seconds": o.seconds, "answer": o.answer }))
        .collect();
    std::fs::write(dir.join("answers.json"), serde_json::to_string_pretty(&saved)?)?;

    for o in &outcomes {
        let mark = if o.problems.is_empty() { "✓" } else { "✗" };
        println!(
            "{mark} {}{} {}",
            cases[o.case].id,
            if args.repeat > 1 { format!("#{}", o.run + 1) } else { String::new() },
            o.problems.join("；")
        );
    }
    let passed = outcomes.iter().filter(|o| o.problems.is_empty()).count();
    let (found, facts) = outcomes.iter().fold((0, 0), |(f, t), o| (f + o.facts.0, t + o.facts.1));
    let count = |prefix: &str| outcomes.iter().filter(|o| o.problems.iter().any(|p| p.starts_with(prefix))).count();
    let seconds = outcomes.iter().map(|o| o.seconds).sum::<f64>() / outcomes.len().max(1) as f64;
    println!(
        "\n{label}：{} 次，通过 {passed}；信息 {found}/{facts}；多写描述 {}；照抄 {}；重复标题 {}；口语 {}；出错 {}；平均 {seconds:.1} 秒\n回答在 {}",
        outcomes.len(),
        count("不该有描述"),
        count("照抄"),
        count("描述重复"),
        count("口语"),
        count("出错"),
        dir.display()
    );
    Ok(())
}

fn ask(config: &Config, system: Option<&str>, case: &Case, index: usize, run: usize) -> Outcome {
    let e = match &case.expect {
        Kind::Organize(e) => e,
        Kind::Tidy(e, data) => return ask_tidy(config, system, e, data, index, run),
    };
    let snap = snapshot(&case.input);
    let shown: HashSet<i64> = snap.tasks.iter().map(|t| t.number).collect();
    let prompt = organize::chat(system.unwrap_or(organize::SYSTEM), &case.input);
    let (answer, seconds) = chat(config, &prompt);
    let facts = e.facts.len();
    let (answer, checked) = match answer {
        Err(e) => (String::new(), Err(format!("出错：{e}"))),
        Ok(text) => {
            // As the server adds them: what it refuses fails the job.
            let planned = organize::parse(&text, &shown)
                .and_then(|tasks| tools::plan_into(&Board::new(&snap), None, tasks).map_err(|e| e.describe()))
                .and_then(|ops| accepted(&snap, &ops).map(|()| ops));
            let checked = match planned {
                Err(e) => Err(format!("出错：{e}")),
                Ok(ops) => Ok(check(e, &case.input, &serde_json::from_str(organize::unwrap(&text)).expect("parsed above"), &snap, &ops)),
            };
            (text, checked)
        }
    };
    let (problems, facts) = match checked {
        Ok((problems, found)) => (problems, (found, facts)),
        Err(e) => (vec![e], (0, facts)),
    };
    Outcome { case: index, run, answer, problems, facts, seconds }
}

/// The model's answer to `prompt`, and how many seconds it took. Turned away for asking too often,
/// or by a provider down for a moment, it asks again a while later: a set of cases asks many times
/// in a row, for minutes.
fn chat(config: &Config, prompt: &Prompt) -> (Result<String, String>, f64) {
    let mut waits = [20, 40, 60].into_iter();
    loop {
        let start = Instant::now();
        // As in local mode, where any address is called: a proxy here may give every name one of its own.
        let answer = Https { public_only: false }.chat(config, prompt, true, TIMEOUT);
        if let Err(f) = &answer
            && f.status.is_some_and(|s| s == 429 || s >= 500)
            && let Some(wait) = waits.next()
        {
            std::thread::sleep(Duration::from_secs(wait));
            continue;
        }
        return (answer.map_err(|f| f.message), start.elapsed().as_secs_f64());
    }
}

/// The case's folder as the server keeps it: its open tasks, and their dependencies.
fn snapshot(input: &Value) -> Snapshot {
    let id = |n: i64| format!("00000000-0000-7000-8000-{n:012}");
    let open: Vec<&Value> = input["open_tasks"].as_array().into_iter().flatten().collect();
    let tasks: Vec<Value> = open
        .iter()
        .map(|t| {
            let n = t["number"].as_i64().expect("open tasks have numbers");
            json!({ "id": id(n), "number": n, "folder_id": null, "title": t["title"], "description": "", "done_at": null, "due": null,
                "sort_key": "a0", "created_at": 0, "updated_at": 0, "created_via": null, "updated_via": null })
        })
        .collect();
    let dependencies: Vec<Value> = open
        .iter()
        .flat_map(|t| t["after"].as_array().into_iter().flatten().map(|b| json!({ "before": id(b.as_i64().unwrap_or_default()), "after": id(t["number"].as_i64().unwrap_or_default()), "created_at": 0 })))
        .collect();
    serde_json::from_value(json!({ "folders": [], "tasks": tasks, "dependencies": dependencies, "ai_jobs": [] })).expect("a snapshot")
}

/// Refuses what the server refuses when it adds the tasks: more operations than a change holds, with
/// the job's deletion; a dependency that closes a cycle, as they are added one by one.
fn accepted(snap: &Snapshot, ops: &[Op]) -> Result<(), String> {
    if ops.len() + 1 > MAX_OPS {
        return Err(format!("每次最多 {MAX_OPS} 个操作，本次有 {} 个", ops.len() + 1));
    }
    let mut edges: Vec<(TaskId, TaskId)> = snap.dependencies.iter().map(|d| (d.before, d.after)).collect();
    for op in ops {
        if let Op::DepAdd { before, after } = op {
            if before == after || find_path(&edges, *after, *before).is_some() {
                return Err("先后关系成环".into());
            }
            edges.push((*before, *after));
        }
    }
    Ok(())
}

/// What is wrong with the answer `v`, which the server would add as `ops`, and how many of the
/// facts it keeps.
fn check(e: &Expect, input: &Value, v: &Value, snap: &Snapshot, ops: &[Op]) -> (Vec<String>, usize) {
    let tasks: Vec<&Value> = v["tasks"].as_array().into_iter().flatten().collect();
    let field = |t: &Value, k: &str| t[k].as_str().map(str::trim).unwrap_or_default().to_owned();
    let all: String = tasks.iter().map(|t| format!("{}\n{}\n", field(t, "title"), field(t, "description"))).collect();
    let flat = squash(&all);
    let mut problems = Vec::new();

    if let Some((least, most)) = e.tasks
        && !(least..=most).contains(&tasks.len())
    {
        problems.push(format!("条数 {}，应为 {least}–{most}", tasks.len()));
    }
    let missing: Vec<&str> =
        e.facts.iter().filter(|alts| !alts.iter().any(|a| flat.contains(&squash(a)))).map(|alts| alts[0].as_str()).collect();
    if !missing.is_empty() {
        problems.push(format!("缺少：{}", missing.join("、")));
    }
    for s in e.verbatim.iter().filter(|s| !all.contains(s.as_str())) {
        problems.push(format!("没有原样保留：{s}"));
    }
    for s in e.absent.iter().filter(|s| flat.contains(&squash(s))) {
        problems.push(format!("不该出现：{s}"));
    }

    let text = input["text"].as_str().unwrap_or_default();
    for t in &tasks {
        let description = field(t, "description");
        if description.is_empty() {
            continue;
        }
        if e.description == Some(false) {
            problems.push(format!("不该有描述：{description}"));
            continue;
        }
        let prose = prose(&description, &e.verbatim);
        if let Some(p) = spoken(&prose) {
            problems.push(p);
        }
        let share = copied(&prose, &prose_of_text(text, &e.verbatim), RUN);
        if !e.written && share >= COPIED && prose.chars().count() >= 2 * RUN {
            problems.push(format!("照抄原话 {:.0}%：{}", share * 100.0, field(t, "title")));
        }
        if copied(&prose, &field(t, "title"), 3) >= REPEATED {
            problems.push(format!("描述重复标题：{description}"));
        }
    }

    if let Some(want) = &e.due {
        let got: Vec<String> = sorted(tasks.iter().map(|t| field(t, "due")).filter(|d| !d.is_empty()));
        if got != sorted(want.iter().cloned()) {
            problems.push(format!("截止时间 {got:?}，应为 {want:?}"));
        }
    }
    if let Some(want) = &e.remind {
        let got: Vec<u32> = sorted(tasks.iter().filter_map(|t| t["remind"].as_u64()).map(|r| r as u32));
        if got != sorted(want.iter().copied()) {
            problems.push(format!("提醒 {got:?}，应为 {want:?}"));
        }
    }
    // The dependencies as added: an open task is known by its number, a new one by its title.
    let created: Vec<(TaskId, String)> = ops
        .iter()
        .filter_map(|op| match op {
            Op::TaskCreate { id, title, .. } => Some((*id, squash(title.as_str()))),
            _ => None,
        })
        .collect();
    let edges: Vec<(TaskId, TaskId)> = ops
        .iter()
        .filter_map(|op| match op {
            Op::DepAdd { before, after } => Some((*before, *after)),
            _ => None,
        })
        .collect();
    let new = |id: &TaskId| created.iter().any(|(c, _)| c == id);
    let number = |id: &TaskId| snap.tasks.iter().find(|t| t.id == *id).map(|t| t.number);
    let after: BTreeSet<i64> = edges.iter().filter(|(_, a)| new(a)).filter_map(|(b, _)| number(b)).collect();
    let before: BTreeSet<i64> = edges.iter().filter(|(b, _)| new(b)).filter_map(|(_, a)| number(a)).collect();
    for (key, want, got) in [("after", &e.after, after), ("before", &e.before, before)] {
        if let Some(want) = want
            && got != want.iter().copied().collect()
        {
            problems.push(format!("{key} {got:?}，应为 {want:?}"));
        }
    }
    if let Some(want) = e.linked
        && edges.iter().any(|(b, a)| new(b) && new(a)) != want
    {
        problems.push(if want { "新事项之间没有连上先后".into() } else { "新事项之间不该连先后".into() });
    }
    let titled = |word: &str| created.iter().find(|(_, t)| t.contains(&squash(word))).map(|(id, _)| *id);
    for pair in e.order.windows(2) {
        match (titled(&pair[0]), titled(&pair[1])) {
            (Some(a), Some(b)) if a == b || find_path(&edges, a, b).is_some() => {}
            (Some(_), Some(_)) => problems.push(format!("“{}”没有排在“{}”之后", pair[1], pair[0])),
            _ => problems.push(format!("标题里找不到“{}”或“{}”", pair[0], pair[1])),
        }
    }
    let found = e.facts.len() - missing.len();
    (problems, found)
}

/// A due date in full, for comparing: `2026-12-01 09:00 !1440 ~1m`, the reminder after `!` and the
/// repeat after `~`, `/done` when counted from the day it is done; empty for none.
fn due_text(d: Option<&Due>) -> String {
    let Some(d) = d else { return String::new() };
    let mut s = d.date.as_str().to_owned();
    if let Some(at) = &d.at {
        s += &format!(" {}", at.time.as_str());
        if let Some(m) = at.remind_before {
            s += &format!(" !{m}");
        }
    }
    if let Some(r) = d.repeat {
        let unit = match r.unit {
            RepeatUnit::Day => 'd',
            RepeatUnit::Week => 'w',
            RepeatUnit::Month => 'm',
        };
        s += &format!(" ~{}{unit}{}", r.every.get(), if r.from == RepeatFrom::Done { "/done" } else { "" });
    }
    s
}

/// The spoken words in `prose`, if any.
fn spoken(prose: &str) -> Option<String> {
    let ends = |i: usize| prose[i..].chars().nth(1).is_none_or(|c| "，。！？；、,.!?;…~\n".contains(c));
    let fillers: Vec<&str> = SPOKEN
        .iter()
        .copied()
        .filter(|w| prose.contains(w))
        .chain(PARTICLES.iter().copied().filter(|p| prose.match_indices(p).any(|(i, _)| ends(i))))
        .collect();
    (!fillers.is_empty()).then(|| format!("口语：{}", fillers.join("、")))
}

fn sorted<T: Ord>(items: impl Iterator<Item = T>) -> Vec<T> {
    let mut v: Vec<T> = items.collect();
    v.sort();
    v
}

/// For finding words in a text whatever the case and the spaces.
fn squash(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect()
}

/// A description without its code, inline or in blocks, and what has to be kept as written: what is
/// left is to be in the description's own words.
fn prose(description: &str, verbatim: &[String]) -> String {
    let mut out = String::new();
    for (i, part) in description.split("```").enumerate() {
        if i % 2 == 0 {
            out.extend(part.split('`').step_by(2));
        }
    }
    verbatim.iter().fold(out, |s, v| s.replace(v.as_str(), ""))
}

fn prose_of_text(text: &str, verbatim: &[String]) -> String {
    verbatim.iter().fold(text.to_owned(), |s, v| s.replace(v.as_str(), ""))
}

/// How much of `prose`, by Chinese characters, lies in runs of at least `run` it shares with `text`.
fn copied(prose: &str, text: &str, run: usize) -> f64 {
    let han = |s: &str| -> Vec<char> { s.chars().filter(|c| ('\u{4e00}'..='\u{9fff}').contains(c)).collect() };
    let (p, t) = (han(prose), han(text));
    let mut covered = vec![false; p.len()];
    for i in 0..p.len() {
        let longest = (0..t.len()).map(|j| p[i..].iter().zip(&t[j..]).take_while(|(a, b)| a == b).count()).max().unwrap_or(0);
        if longest >= run {
            covered[i..i + longest].iter_mut().for_each(|c| *c = true);
        }
    }
    covered.iter().filter(|&&c| c).count() as f64 / p.len().max(1) as f64
}

/// The data a tidying case shows: its folder and subfolders, their tasks in the order listed, done
/// ones done in it, and the job.
fn tidy_data(input: &Value) -> (Snapshot, AiJob) {
    let id = |n: usize| format!("00000000-0000-7000-8000-{n:012}");
    let mut folders = Vec::new();
    let mut paths: HashMap<String, String> = HashMap::new();
    let list = |k: &str| input[k].as_array().cloned().unwrap_or_default();
    let mut folder_of = |path: &str, folders: &mut Vec<Value>| -> Option<String> {
        let mut parent: Option<String> = None;
        let mut full = String::new();
        for name in path.split('/').filter(|s| !s.is_empty()) {
            full = format!("{full}/{name}");
            let next = paths.len() + 1000;
            let f = paths.entry(full.clone()).or_insert_with(|| {
                folders
                    .push(json!({ "id": id(next), "parent_id": parent, "name": name, "sort_key": "a0", "created_at": 0, "updated_at": 0 }));
                id(next)
            });
            parent = Some(f.clone());
        }
        parent
    };
    let folder = folder_of(input["folder"].as_str().expect("a folder"), &mut folders);
    for p in list("subfolders") {
        folder_of(p.as_str().expect("folder paths"), &mut folders);
    }
    let task_id = |n: i64| id(n as usize);
    let mut tasks = Vec::new();
    let mut dependencies = Vec::new();
    let open = list("open_tasks");
    let done = list("done_tasks");
    for (i, t) in open.iter().chain(&done).enumerate() {
        let n = t["number"].as_i64().expect("tasks have numbers");
        let due = t["due"].as_str().map(|d| {
            let (date, time) = d.split_once(' ').map_or((d, None), |(d, t)| (d, Some(t)));
            let repeat = t["repeat"].as_str().map(|r| {
                let (every, unit) = r.split_at(r.len() - 1);
                let unit = match unit {
                    "d" => "day",
                    "w" => "week",
                    _ => "month",
                };
                json!({ "every": every.parse::<u32>().expect("a number"), "unit": unit, "from": t["repeat_from"].as_str().unwrap_or("due") })
            });
            json!({ "date": date, "time": time, "remind_before": t["remind"], "repeat": repeat })
        });
        let due = due.map(|mut d| {
            d.as_object_mut().unwrap().retain(|_, v| !v.is_null());
            d
        });
        tasks.push(json!({
            "id": task_id(n), "number": n, "folder_id": folder, "title": t["title"], "description": t["description"].as_str().unwrap_or_default(),
            // Listed latest first.
            "done_at": (i >= open.len()).then(|| 10_000 - i), "due": due, "sort_key": format!("a{i:02}1"),
            "created_at": 0, "updated_at": 0, "created_via": null, "updated_via": null,
        }));
        for b in t["requires"].as_array().into_iter().flatten() {
            dependencies.push(json!({ "before": task_id(b.as_i64().expect("numbers")), "after": task_id(n), "created_at": 0 }));
        }
    }
    let snap = serde_json::from_value(json!({ "folders": folders, "tasks": tasks, "dependencies": dependencies, "ai_jobs": [] }))
        .expect("a snapshot");
    let job = serde_json::from_value(json!({
        "id": id(999), "folder_id": folder, "kind": "tidy", "text": input["request"], "local_time": input["local_time"],
        "state": "running", "error": null, "created_at": 0, "updated_at": 0,
    }))
    .expect("a job");
    (snap, job)
}

fn ask_tidy(config: &Config, system: Option<&str>, e: &TidyExpect, data: &(Snapshot, AiJob), index: usize, run: usize) -> Outcome {
    let (snap, job) = data;
    let (mut prompt, shown) = tidy::prompt(job, snap.clone());
    if let Some(s) = system {
        prompt.system = s.to_owned();
    }
    let (answer, seconds) = chat(config, &prompt);
    let facts = e.facts.len();
    let (answer, checked) = match answer {
        Err(e) => (String::new(), Err(format!("出错：{e}"))),
        Ok(text) => {
            let checked = tidy::parse(&text, &shown).map_err(|e| format!("出错：{e}")).map(|s| check_tidy(e, snap, &s));
            (text, checked)
        }
    };
    let (problems, facts) = match checked {
        Ok((problems, found)) => (problems, (found, facts)),
        Err(e) => (vec![e], (0, facts)),
    };
    Outcome { case: index, run, answer, problems, facts, seconds }
}

/// What is wrong with the suggestions `s` for `snap`, and how many of the facts all of them taken keep.
fn check_tidy(e: &TidyExpect, snap: &Snapshot, s: &[Suggestion]) -> (Vec<String>, usize) {
    let b = Board::new(snap);
    let number = |id: &TaskId| b.task(*id).map(|t| t.number);
    let mut problems = Vec::new();
    let mut kinds = BTreeSet::new();
    let (mut deleted, mut done, mut reopened, mut moved, mut due, mut new_due, mut touched) =
        (vec![], vec![], vec![], vec![], vec![], vec![], vec![]);
    let mut texts: HashMap<TaskId, (String, String)> =
        snap.tasks.iter().map(|t| (t.id, (t.title.as_str().into(), t.description.as_str().into()))).collect();
    let mut edges: HashSet<(TaskId, TaskId)> = snap.dependencies.iter().map(|d| (d.before, d.after)).collect();
    let mut created = Vec::new();
    let mut rewritten = Vec::new();
    let mut order = Vec::new();
    for x in s {
        match x {
            Suggestion::Edit(t) => {
                let n = number(&t.task).expect("edits are of tasks there");
                touched.push(n);
                let text = texts.get_mut(&t.task).expect("tasks there have texts");
                for (kind, on) in [
                    ("title", t.title.is_some()),
                    ("description", t.description.is_some()),
                    ("due", t.due.is_some()),
                    ("done", t.done == Some(true)),
                    ("reopen", t.done == Some(false)),
                    ("folder", t.folder.is_some()),
                    ("after", !t.after.is_empty()),
                    ("unlink", !t.unlink.is_empty()),
                ] {
                    if on {
                        kinds.insert(kind);
                    }
                }
                if let Some(x) = &t.title {
                    text.0 = x.as_str().into();
                }
                if let Some(x) = &t.description {
                    text.1 = x.as_str().into();
                    rewritten.push(x.as_str().to_owned());
                }
                if let Some(d) = &t.due {
                    due.push((n, due_text(d.as_ref())));
                }
                match t.done {
                    Some(true) => done.push(n),
                    Some(false) => reopened.push(n),
                    None => {}
                }
                if let Some(f) = t.folder {
                    moved.push((n, b.path(Some(f))));
                }
                edges.extend(t.after.iter().map(|a| (*a, t.task)));
                for u in &t.unlink {
                    edges.remove(&(*u, t.task));
                }
            }
            Suggestion::Delete(d) => {
                kinds.insert("delete");
                let n = number(&d.delete).expect("deletions are of tasks there");
                touched.push(n);
                deleted.push(n);
                texts.remove(&d.delete);
                edges.retain(|(x, y)| *x != d.delete && *y != d.delete);
            }
            Suggestion::Create(c) => {
                kinds.insert("create");
                texts.insert(c.create, (c.title.as_str().into(), c.description.as_ref().map_or("", |d| d.as_str()).into()));
                if let Some(d) = &c.description {
                    rewritten.push(d.as_str().to_owned());
                }
                created.push(c.create);
                if let Some(d) = &c.due {
                    new_due.push((c.title.as_str().to_owned(), due_text(Some(d))));
                }
                edges.extend(c.after.iter().map(|a| (*a, c.create)).chain(c.before.iter().map(|x| (c.create, *x))));
            }
            Suggestion::Order(o) => {
                kinds.insert("order");
                order = o.order.iter().filter_map(number).collect();
                // Those that change places among themselves.
                let listed: Vec<i64> = b.tasks_in(snap.tasks[0].folder_id).iter().map(|t| t.number).filter(|n| order.contains(n)).collect();
                touched.extend(order.iter().zip(&listed).filter(|(a, b)| a != b).map(|(a, _)| *a));
            }
        }
    }

    if let Some(allowed) = &e.allowed {
        let wrong: Vec<&str> = kinds.iter().copied().filter(|k| !allowed.iter().any(|a| a == k)).collect();
        if !wrong.is_empty() {
            problems.push(format!("不该做：{}", wrong.join("、")));
        }
    }
    for (what, want, got) in [("删除", &e.deleted, &deleted), ("完成", &e.done, &done), ("重新打开", &e.reopened, &reopened)] {
        if let Some(want) = want
            && sorted(want.iter().copied()) != sorted(got.iter().copied())
        {
            problems.push(format!("{what} {got:?}，应为 {want:?}"));
        }
    }
    if let Some(n) = e.deletes
        && deleted.len() != n
    {
        problems.push(format!("删除了 {} 项，应为 {n} 项", deleted.len()));
    }
    for (what, want, got) in [("移动", &e.moved, &moved), ("截止时间", &e.due, &due)] {
        if let Some(want) = want
            && sorted(want.iter().cloned()) != sorted(got.iter().cloned())
        {
            problems.push(format!("{what} {got:?}，应为 {want:?}"));
        }
    }
    for (word, want) in &e.new_due {
        match new_due.iter().find(|(t, _)| squash(t).contains(&squash(word))) {
            Some((_, got)) if got == want => {}
            got => problems.push(format!("新事项“{word}”的截止时间 {:?}，应为 {want:?}", got.map(|g| &g.1))),
        }
    }
    // The tasks a word names: those there once everything is taken with it in their titles.
    let who = |w: &Who| -> Vec<TaskId> {
        match w {
            Who::Number(n) => b.task_by_number(*n).map(|t| t.id).into_iter().collect(),
            Who::Words(w) => texts.iter().filter(|(_, (t, _))| squash(t).contains(&squash(w))).map(|(id, _)| *id).collect(),
        }
    };
    let named = |w: &Who| match w {
        Who::Number(n) => format!("#{n}"),
        Who::Words(w) => format!("“{w}”"),
    };
    let edges: Vec<(TaskId, TaskId)> = edges.into_iter().collect();
    // Whether one of `x` comes before another one of `y`.
    let before = |x: &Who, y: &Who| {
        let (xs, ys) = (who(x), who(y));
        xs.iter().any(|a| ys.iter().any(|b| a != b && find_path(&edges, *a, *b).is_some()))
    };
    for (want, (x, y)) in e.linked.iter().map(|p| (true, p)).chain(e.not_linked.iter().map(|p| (false, p))) {
        if who(x).is_empty() || who(y).is_empty() {
            problems.push(format!("找不到 {} 或 {}", named(x), named(y)));
        } else if before(x, y) != want {
            problems.push(if want {
                format!("{} 没有设为 {} 的前置", named(x), named(y))
            } else {
                format!("{} 不该在 {} 之前", named(x), named(y))
            });
        }
    }
    for (x, y) in &e.unlinked {
        if matches!((b.task_by_number(*x), b.task_by_number(*y)), (Some(x), Some(y)) if edges.contains(&(x.id, y.id))) {
            problems.push(format!("#{x} 仍是 #{y} 的前置"));
        }
    }
    if let Some((least, most)) = e.created
        && !(least..=most).contains(&created.len())
    {
        problems.push(format!("新建 {} 项，应为 {least}–{most}", created.len()));
    }
    let busy: Vec<i64> = e.untouched.iter().copied().filter(|n| touched.contains(n)).collect();
    if !busy.is_empty() {
        problems.push(format!("不该动：{busy:?}"));
    }
    if !e.order.is_empty() {
        let got: Vec<i64> = order.iter().copied().filter(|n| e.order.contains(n)).collect();
        if got != e.order {
            problems.push(format!("顺序 {got:?}，应为 {:?}", e.order));
        }
    }
    let all: String = texts.values().map(|(t, d)| format!("{t}\n{d}\n")).collect();
    let flat = squash(&all);
    let missing: Vec<&str> =
        e.facts.iter().filter(|alts| !alts.iter().any(|a| flat.contains(&squash(a)))).map(|alts| alts[0].as_str()).collect();
    if !missing.is_empty() {
        problems.push(format!("缺少：{}", missing.join("、")));
    }
    for v in e.verbatim.iter().filter(|v| !all.contains(v.as_str())) {
        problems.push(format!("没有原样保留：{v}"));
    }
    for a in e.absent.iter().filter(|a| flat.contains(&squash(a))) {
        problems.push(format!("不该出现：{a}"));
    }
    problems.extend(rewritten.iter().filter_map(|d| spoken(&prose(d, &e.verbatim))));
    (problems, e.facts.len() - missing.len())
}
