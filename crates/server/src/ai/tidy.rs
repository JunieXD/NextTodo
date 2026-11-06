//! Tidying a folder's open tasks: what a job asks the model, and reading back the changes it
//! suggests, which the user then takes or leaves one by one.

use std::collections::{HashMap, HashSet};

use nexttodo_core::board::{Board, Snapshot};
use nexttodo_core::dag::find_path;
use nexttodo_core::model::{AiJob, Description, Due, Suggestion, Task, TaskId, Title, Was};
use nexttodo_core::ops::MAX_OPS;
use serde::Deserialize;
use serde_json::{Value, json};

use super::llm::Prompt;
use super::organize::{due_text, unwrap};
use crate::error::from_value;
use crate::tools::args::DueText;

/// How many of the folder's open tasks the model is shown.
const SHOWN: usize = 50;
/// How much of their descriptions, in characters: tasks past it go without theirs.
const DESCRIBED: usize = 30_000;
/// How many links may be suggested: all taken, they go in one change with an update of each task
/// and the job's deletion.
const LINKS: usize = MAX_OPS - SHOWN - 1;

const SYSTEM: &str = r#"你负责整理一个文件夹里未完成的待办事项：提出修改建议，由用户逐条确认后再改。输入是一个 JSON 对象：
- local_time：用户提交时的本地时间；
- folder：文件夹；
- request：用户这次的要求；
- open_tasks：文件夹里未完成的事项，number 是编号，after 是它已有的前置事项的编号；description_omitted 为 true 的事项描述太长，没有发来。

只输出一个 json 对象，不要有任何别的文字，格式如下：
{"suggestions": [{"number": 12, "title": "…", "description": "…", "due": "2026-11-20 18:00", "after": [3], "reason": "…"}]}

字段：
- number：必填，要修改的事项的编号；每个事项最多出现一次。
- title：选填，新标题，一行，简短（最好不超过 30 个字），以动作开头。
- description：选填，新描述，是改好后的完整内容，不是修改说明；Markdown，步骤可以写成清单“- [ ] ”。
- due：选填，截止时间，"YYYY-MM-DD" 或 "YYYY-MM-DD HH:MM"。
- after：选填，要新加的前置事项的编号，它们完成后才能做这一项；已有的不用写。
- reason：必填，一句话说明改了什么、为什么。
- 不改的字段省略，不要写空字符串；不需要改的事项不要列出。

整理时：
1. 按 request 的要求来。没有具体要求时，看标题是否清楚、描述是否是简洁的书面语、有没有漏掉的先后关系。
2. 改写不能丢信息：原标题和描述里的每一条要求、细节、理由、背景和疑问，都要留在新标题或新描述里，不要概括掉；不要编造原文没有的内容。改了标题时，原标题里放不下的细节写进描述。
3. 描述常是语音输入的口语。改写成简洁的书面语：去掉语气词、重复、说到一半的改口和“我觉得”“就是说”这类口头填充，但不能因此丢掉信息。
4. 写给 AI 或 agent 的提示词、代码、命令、链接、引文和数据，原样保留，一个字都不改。
5. 一个事项要用到另一个事项的成果，或要等它做完才能开始，就用 after 把那个事项设为前置，例如“写官网文档”要等“建好官网”。只是主题相近、没有先后的，不连；不要连成环。
6. 只有标题或描述里明确写了时间、事项又还没有截止时间时，才写 due，“周五前”这类按 local_time 换算成具体日期。
7. description_omitted 为 true 的事项只能改 due 和 after，不要写 title 和 description：没看到描述，改标题可能丢掉信息。
8. 已经清楚的事项不要列出，不要为改而改。
9. 用原文的语言写。"#;

/// The tasks the model was shown, by number, and every dependency, before and after.
pub struct Shown {
    tasks: HashMap<i64, Sent>,
    edges: Vec<(TaskId, TaskId)>,
}

struct Sent {
    task: Task,
    /// Whether its description was sent: only then may the model retitle it or rewrite that.
    described: bool,
}

/// The chat for `job`, a tidying of its folder, and what was shown in it.
pub fn prompt(job: &AiJob, snap: &Snapshot) -> (Prompt, Shown) {
    let board = Board::new(snap);
    let open: Vec<&Task> = board.tasks_in(job.folder_id).into_iter().filter(|t| t.done_at.is_none()).take(SHOWN).collect();
    let numbers: HashSet<i64> = open.iter().map(|t| t.number).collect();
    let mut budget = DESCRIBED;
    let mut tasks = HashMap::new();
    let listed: Vec<Value> = open
        .iter()
        .map(|t| {
            let mut v = json!({ "number": t.number, "title": t.title.as_str() });
            let text = t.description.as_str();
            let described = text.chars().count() <= budget;
            if !described {
                v["description_omitted"] = true.into();
            } else if !text.is_empty() {
                budget -= text.chars().count();
                v["description"] = text.into();
            }
            if let Some(d) = &t.due {
                v["due"] = due_text(d).into();
            }
            let after: Vec<i64> = board.before(t).iter().map(|b| b.number).filter(|n| numbers.contains(n)).collect();
            if !after.is_empty() {
                v["after"] = after.into();
            }
            tasks.insert(t.number, Sent { task: (*t).clone(), described });
            v
        })
        .collect();
    let input = json!({
        "local_time": job.local_time.as_str(),
        "folder": board.path(job.folder_id),
        "request": job.text.as_str(),
        "open_tasks": listed,
    });
    let edges = snap.dependencies.iter().map(|d| (d.before, d.after)).collect();
    (Prompt { system: SYSTEM.into(), user: serde_json::to_string_pretty(&input).expect("JSON values serialize") }, Shown { tasks, edges })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    suggestions: Vec<Suggested>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Suggested {
    number: i64,
    title: Option<Title>,
    description: Option<Description>,
    due: Option<DueText>,
    #[serde(default)]
    after: Vec<i64>,
    reason: String,
}

/// The changes the model suggests, to tasks it was shown, leaving out what would change nothing.
pub fn parse(answer: &str, shown: &Shown) -> Result<Vec<Suggestion>, String> {
    let mut v: Value = serde_json::from_str(unwrap(answer)).map_err(|_| "模型返回的不是合法的 JSON".to_owned())?;
    // An empty field is one the model has nothing for, as in organizing.
    for s in v["suggestions"].as_array_mut().into_iter().flatten() {
        if let Some(fields) = s.as_object_mut() {
            fields.retain(|k, f| k == "reason" || !(f.is_null() || f.as_str().is_some_and(|s| s.trim().is_empty())));
        }
    }
    let Answer { suggestions } = from_value(v).map_err(|e| format!("模型返回的结果不符合格式：{}", e.describe()))?;
    let mut edges = shown.edges.clone();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for s in suggestions {
        let Some(sent) = shown.tasks.get(&s.number) else {
            return Err(format!("结果引用了不在这个文件夹里的事项 #{}", s.number));
        };
        if !seen.insert(s.number) {
            return Err(format!("结果里事项 #{} 出现了不止一次", s.number));
        }
        let t = &sent.task;
        let due = match s.due {
            Some(d) => Some(Due::new(d.date, d.time, None).map_err(|m| format!("事项 #{} 的截止时间不对：{m}", s.number))?),
            None => None,
        };
        let mut after = Vec::new();
        for n in s.after {
            let Some(before) = shown.tasks.get(&n) else { return Err(format!("结果引用了不在这个文件夹里的事项 #{n}")) };
            let b = before.task.id;
            if edges.contains(&(b, t.id)) {
                continue;
            }
            if find_path(&edges, t.id, b).is_some() {
                return Err(format!("建议把 #{n} 设为 #{} 的前置，但这样依赖会成环", s.number));
            }
            edges.push((b, t.id));
            after.push(b);
        }
        if edges.len() - shown.edges.len() > LINKS {
            return Err(format!("建议加的前置太多，超过 {LINKS} 条，请缩小范围再试"));
        }
        let suggestion = Suggestion {
            task: t.id,
            // Not having seen the description, it would lose what the title held that this lacks.
            title: s.title.filter(|x| sent.described && *x != t.title),
            description: s.description.filter(|x| sent.described && *x != t.description),
            due: due.filter(|d| t.due.as_ref() != Some(d)),
            after,
            reason: s.reason,
            was: Was { title: t.title.clone(), description: t.description.clone(), due: t.due.clone() },
        };
        if suggestion.title.is_some() || suggestion.description.is_some() || suggestion.due.is_some() || !suggestion.after.is_empty() {
            out.push(suggestion);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexttodo_core::model::Dependency;

    fn snap() -> Snapshot {
        let task = |n: i64, title: &str, description: &str| -> Task {
            serde_json::from_value(json!({
                "id": format!("00000000-0000-7000-8000-{n:012}"), "number": n, "folder_id": null, "title": title,
                "description": description, "done_at": null, "due": null, "sort_key": format!("a{n}"),
                "created_at": 0, "updated_at": 0, "created_via": null, "updated_via": null,
            }))
            .unwrap()
        };
        // The last two are too long together: the second goes without its description.
        let long = "字".repeat(18_000);
        let tasks = vec![task(1, "官网", "就是说要做一个官网"), task(2, "写官网文档", ""), task(3, "长的", &long), task(4, "也长", &long)];
        let dependencies = vec![Dependency { before: tasks[0].id, after: tasks[1].id, created_at: 0 }];
        Snapshot { tasks, dependencies, ..Default::default() }
    }

    fn job() -> AiJob {
        serde_json::from_value(json!({
            "id": "00000000-0000-7000-8000-000000000099", "folder_id": null, "kind": "tidy", "text": "整理一下",
            "local_time": "2026-11-07 10:00 星期六", "state": "running", "error": null, "created_at": 0, "updated_at": 0,
        }))
        .unwrap()
    }

    #[test]
    fn shows_the_open_tasks_within_the_budget() {
        let (prompt, shown) = prompt(&job(), &snap());
        let input: Value = serde_json::from_str(&prompt.user).unwrap();
        assert_eq!(input["request"], "整理一下");
        assert_eq!(input["open_tasks"][1]["after"], json!([1]));
        assert!(shown.tasks[&3].described);
        assert_eq!(input["open_tasks"][3]["description_omitted"], true);
        assert!(input["open_tasks"][3].get("description").is_none());
        assert!(!shown.tasks[&4].described);
    }

    #[test]
    fn checks_the_suggestions() {
        let (_, shown) = prompt(&job(), &snap());
        let ok = r#"{"suggestions": [
            {"number": 1, "title": "建好官网", "description": "做一个官网。", "due": "2026-11-20", "reason": "标题以动作开头"},
            {"number": 2, "title": "写官网文档", "after": [1], "reason": "已有的不变"},
            {"number": 4, "title": "短", "description": "短了", "after": [2], "reason": "描述没发来"}
        ]}"#;
        let s = parse(ok, &shown).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].title.as_ref().unwrap().as_str(), "建好官网");
        assert_eq!(s[0].due.as_ref().unwrap().date.as_str(), "2026-11-20");
        assert!(s[1].title.is_none() && s[1].description.is_none() && s[1].after.len() == 1);
        assert_eq!(s[0].was.title.as_str(), "官网");
        assert_eq!(parse(r#"{"suggestions": []}"#, &shown).unwrap().len(), 0);
        assert_eq!(
            parse(r#"{"suggestions": [{"number": 9, "title": "x", "reason": ""}]}"#, &shown).err().unwrap(),
            "结果引用了不在这个文件夹里的事项 #9"
        );
        let twice = r#"{"suggestions": [{"number": 1, "title": "a", "reason": ""}, {"number": 1, "title": "b", "reason": ""}]}"#;
        assert_eq!(parse(twice, &shown).err().unwrap(), "结果里事项 #1 出现了不止一次");
        let cycle = r#"{"suggestions": [{"number": 1, "after": [2], "reason": ""}]}"#;
        assert_eq!(parse(cycle, &shown).err().unwrap(), "建议把 #2 设为 #1 的前置，但这样依赖会成环");
        assert!(parse(r#"{"suggestions": [{"number": 1, "title": "a"}]}"#, &shown).is_err());
    }

    #[test]
    fn suggests_no_more_links_than_one_change_takes() {
        let tasks: Vec<Task> = (1..=50)
            .map(|n| {
                serde_json::from_value(json!({
                    "id": format!("00000000-0000-7000-8000-{n:012}"), "number": n, "folder_id": null, "title": format!("t{n}"),
                    "description": "", "done_at": null, "due": null, "sort_key": format!("a{n:02}1"),
                    "created_at": 0, "updated_at": 0, "created_via": null, "updated_via": null,
                }))
                .unwrap()
            })
            .collect();
        let (_, shown) = prompt(&job(), &Snapshot { tasks, ..Default::default() });
        // Each task after every one before it: 1225 links, a DAG still.
        let all: Vec<Value> = (2..=50).map(|n| json!({ "number": n, "after": (1..n).collect::<Vec<_>>(), "reason": "" })).collect();
        let err = parse(&json!({ "suggestions": all }).to_string(), &shown).err().unwrap();
        assert_eq!(err, format!("建议加的前置太多，超过 {LINKS} 条，请缩小范围再试"));
    }
}
