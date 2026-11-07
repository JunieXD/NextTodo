//! Tries the instructions for AI organizing on a set of cases with a real model, and checks each
//! answer against what its case expects. The model and its key are given in the environment; how,
//! and what is checked, in docs/ai.md#评测.
//!
//! The answers go to target/ai-eval, for reading them through.

use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::Parser;
use nexttodo_core::board::{Board, Snapshot};
use nexttodo_core::dag::find_path;
use nexttodo_core::model::TaskId;
use nexttodo_core::ops::Op;
use nexttodo_server::ai::llm::{self, Config, Prompt, Thinking};
use nexttodo_server::ai::organize;
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
struct Case {
    id: String,
    /// What it tries, for people.
    #[serde(default)]
    #[allow(dead_code)]
    note: String,
    /// What the server gives the model: local_time, folder, open_tasks and text.
    input: Value,
    expect: Expect,
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
    let system = match &args.system {
        Some(path) => std::fs::read_to_string(path)?,
        None => organize::SYSTEM.to_owned(),
    };
    let mut cases = Vec::new();
    for path in &args.cases {
        let read: Vec<Case> =
            serde_json::from_str(&std::fs::read_to_string(path)?).map_err(|e| anyhow::anyhow!("{}：{e}", path.display()))?;
        cases.extend(read);
    }
    cases.retain(|c| args.only.as_ref().is_none_or(|only| c.id.contains(only.as_str())));

    let work: Vec<(usize, usize)> = (0..args.repeat).flat_map(|run| (0..cases.len()).map(move |case| (case, run))).collect();
    let next = AtomicUsize::new(0);
    let outcomes = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..args.jobs.max(1) {
            s.spawn(|| {
                while let Some(&(case, run)) = work.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let outcome = ask(&config, &system, &cases[case], case, run);
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

fn ask(config: &Config, system: &str, case: &Case, index: usize, run: usize) -> Outcome {
    let snap = snapshot(&case.input);
    let shown: HashSet<i64> = snap.tasks.iter().map(|t| t.number).collect();
    let start = Instant::now();
    let answer = chat(config, &organize::chat(system, &case.input));
    let seconds = start.elapsed().as_secs_f64();
    let facts = case.expect.facts.len();
    let (answer, checked) = match answer {
        Err(e) => (String::new(), Err(format!("出错：{e}"))),
        Ok(text) => {
            // As the server adds them: what it refuses fails the job.
            let planned = organize::parse(&text, &shown)
                .and_then(|tasks| tools::plan_into(&Board::new(&snap), None, tasks).map_err(|e| e.describe()))
                .and_then(|ops| acyclic(&snap, &ops).map(|()| ops));
            let checked = match planned {
                Err(e) => Err(format!("出错：{e}")),
                Ok(ops) => Ok(check(case, &serde_json::from_str(organize::unwrap(&text)).expect("parsed above"), &snap, &ops)),
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

/// The model's answer, asked as llm.rs asks it but in one piece, and without its check that the
/// address is public: a proxy on the machine running this may give every name an address of its own.
fn chat(config: &Config, prompt: &Prompt) -> Result<String, String> {
    let mut body = llm::body(config, config.check()?, prompt, true);
    body["stream"] = false.into();
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(TIMEOUT)).http_status_as_error(false).build().into();
    let mut res = agent
        .post(format!("{}/chat/completions", config.base_url.trim_end_matches('/')))
        .header("Authorization", format!("Bearer {}", config.api_key))
        .content_type("application/json")
        .send(body.to_string())
        .map_err(|e| e.to_string())?;
    let status = res.status().as_u16();
    let text = res.body_mut().read_to_string().map_err(|e| e.to_string())?;
    if status != 200 {
        return Err(format!("接口返回 {status}：{}", text.replace(&config.api_key, "***").chars().take(300).collect::<String>()));
    }
    let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    v["choices"][0]["message"]["content"].as_str().map(str::to_owned).ok_or_else(|| "回复里没有内容".into())
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

/// Refuses a dependency that closes a cycle, as the server does when it adds them one by one.
fn acyclic(snap: &Snapshot, ops: &[Op]) -> Result<(), String> {
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
fn check(case: &Case, v: &Value, snap: &Snapshot, ops: &[Op]) -> (Vec<String>, usize) {
    let e = &case.expect;
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

    let text = case.input["text"].as_str().unwrap_or_default();
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
        let ends = |i: usize| prose[i..].chars().nth(1).is_none_or(|c| "，。！？；、,.!?;…~\n".contains(c));
        let fillers: Vec<&str> = SPOKEN
            .iter()
            .copied()
            .filter(|w| prose.contains(w))
            .chain(PARTICLES.iter().copied().filter(|p| prose.match_indices(p).any(|(i, _)| ends(i))))
            .collect();
        if !fillers.is_empty() {
            problems.push(format!("口语：{}", fillers.join("、")));
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
