//! What a job asks the model, and reading back the tasks it answers with.

use std::collections::HashSet;

use nexttodo_core::board::Board;
use nexttodo_core::model::{AiJob, Due, Task};
use serde::Deserialize;
use serde_json::{Value, json};

use super::llm::Prompt;
use crate::error::from_value;
use crate::tools::args::{PlanRef, PlanTask};
use crate::tools::number;

/// How many of the folder's open tasks the model is shown.
const SHOWN: usize = 50;

const SYSTEM: &str = r#"你负责把用户随手写下的一段话整理成待办事项。输入是一个 JSON 对象：
- local_time：用户提交时的本地时间；
- folder：这些事项要放进的文件夹；
- open_tasks：这个文件夹里已有的未完成事项，number 是编号，after 是它的前置事项的编号；
- text：用户写的原文。

只输出一个 json 对象，不要有任何别的文字，格式如下：
{"tasks": [{"ref": "a", "title": "…", "description": "…", "due": "2026-11-20 18:00", "remind": 30, "after": [], "before": []}]}

字段：
- title：必填，一行，简短（最好不超过 30 个字），以动作开头，如“写开题报告初稿”。
- description：选填，Markdown，放标题装不下的细节；步骤可以写成清单“- [ ] ”。
- due：选填，截止时间，"YYYY-MM-DD" 或 "YYYY-MM-DD HH:MM"。
- remind：只有原文明确要求提醒时才写：提前多少分钟提醒，0 表示截止时；只能和带具体时间的 due 一起用。
- 选填的字段没有内容时省略，不要写空字符串。
- ref：选填，这次结果里的临时名称，如 "a"、"b"，供 after、before 引用；不能是数字。
- after：选填，前置事项，它们完成后才能做这一项：这次结果里的 ref，或 open_tasks 里的编号（数字）。
- before：选填，后续事项，这一项完成后才能做它们；取值同 after。

整理时：
1. 原文的每一条信息都要保留在某个事项的标题或描述里，一点不丢：要求、细节、理由、背景、疑问都写进描述，尽量用原话，宁可多写也不要概括掉；不要编造原文没有的内容。
2. 原文里写给 AI 或 agent 的提示词、代码、命令、链接、引文和数据，原样放进描述，一个字都不改；提示词和代码用代码块包起来。
3. 原文说的是几件能分开做的事，就拆成几条，有先后的用 after 或 before 连起来；说的是一件事，就只输出一条，不要硬拆。
4. 和 open_tasks 里的事项有明确的先后关系时，可以用编号连上；没有就不连。
5. 只有原文明确说了时间才写 due，“明天下午三点”“周五前”这类按 local_time 换算成具体日期；原文没说要提醒就不写 remind。
6. 用原文的语言写。"#;

fn due_text(d: &Due) -> String {
    match &d.at {
        Some(at) => format!("{} {}", d.date.as_str(), at.time.as_str()),
        None => d.date.as_str().to_owned(),
    }
}

/// The chat for `job`, and the numbers of the tasks it shows, the only ones its tasks may follow or
/// come before.
pub fn prompt(job: &AiJob, board: &Board) -> (Prompt, HashSet<i64>) {
    let open: Vec<&Task> = board.tasks_in(job.folder_id).into_iter().filter(|t| t.done_at.is_none()).take(SHOWN).collect();
    let shown: HashSet<i64> = open.iter().map(|t| t.number).collect();
    let tasks: Vec<Value> = open
        .iter()
        .map(|t| {
            let mut v = json!({ "number": t.number, "title": t.title.as_str() });
            if let Some(d) = &t.due {
                v["due"] = due_text(d).into();
            }
            let after: Vec<i64> = board.before(t).iter().map(|b| b.number).filter(|n| shown.contains(n)).collect();
            if !after.is_empty() {
                v["after"] = after.into();
            }
            v
        })
        .collect();
    let input = json!({
        "local_time": job.local_time.as_str(),
        "folder": board.path(job.folder_id),
        "open_tasks": tasks,
        "text": job.text.as_str(),
    });
    (Prompt { system: SYSTEM.into(), user: serde_json::to_string_pretty(&input).expect("JSON values serialize") }, shown)
}

/// A minimal chat that answers in JSON, to see whether a config works.
pub fn test_prompt() -> Prompt {
    Prompt { system: r#"只输出这个 json 对象：{"ok": true}"#.into(), user: "测试".into() }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    tasks: Vec<PlanTask>,
}

/// The answer without what models put around JSON: their thinking, and a code fence.
pub fn unwrap(answer: &str) -> &str {
    let mut s = answer.trim();
    if let Some(rest) = s.strip_prefix("<think>") {
        s = rest.split_once("</think>").map_or("", |(_, after)| after).trim();
    }
    if let Some(rest) = s.strip_prefix("```") {
        let rest = rest.strip_prefix("json").unwrap_or(rest).trim();
        s = rest.strip_suffix("```").unwrap_or(rest).trim();
    }
    s
}

/// The tasks in the model's answer, linked only to each other and to tasks in `shown`.
pub fn parse(answer: &str, shown: &HashSet<i64>) -> Result<Vec<PlanTask>, String> {
    let mut v: Value = serde_json::from_str(unwrap(answer)).map_err(|_| "模型返回的不是合法的 JSON".to_owned())?;
    // Models write an empty field for one they have nothing for, `"due": ""` say: it is left out. The
    // title has to be there.
    for t in v["tasks"].as_array_mut().into_iter().flatten() {
        if let Some(fields) = t.as_object_mut() {
            fields.retain(|k, f| k == "title" || !(f.is_null() || f.as_str().is_some_and(|s| s.trim().is_empty())));
        }
    }
    let Answer { tasks } = from_value(v).map_err(|e| format!("模型返回的结果不符合格式：{}", e.describe()))?;
    if tasks.is_empty() {
        return Err("模型没有整理出事项".into());
    }
    for t in &tasks {
        if t.folder.is_some() {
            return Err("模型返回的结果指定了文件夹".into());
        }
        for r in t.after.iter().chain(&t.before) {
            let n = match r {
                PlanRef::Number(n) => Some(*n),
                PlanRef::Name(s) => number(s),
            };
            if let Some(n) = n.filter(|n| !shown.contains(n)) {
                return Err(format!("结果引用了不在这个文件夹里的事项 #{n}"));
            }
        }
    }
    Ok(tasks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_json_out_of_thinking_and_fences() {
        assert_eq!(unwrap("<think>先想想</think>\n```json\n{\"tasks\": []}\n```"), "{\"tasks\": []}");
        assert_eq!(unwrap("```\n{}\n```"), "{}");
        assert_eq!(unwrap(" {} "), "{}");
    }

    #[test]
    fn checks_the_answer() {
        let shown = HashSet::from([12]);
        let ok = r##"{"tasks": [{"ref": "a", "title": "写初稿"}, {"title": "改稿", "after": ["a", 12, "#12"]}]}"##;
        assert_eq!(parse(ok, &shown).unwrap().len(), 2);
        assert_eq!(parse("好的", &shown).err().unwrap(), "模型返回的不是合法的 JSON");
        assert_eq!(parse(r#"{"tasks": []}"#, &shown).err().unwrap(), "模型没有整理出事项");
        assert!(parse(r#"{"tasks": [{"title": " "}]}"#, &shown).err().unwrap().starts_with("模型返回的结果不符合格式：标题不能为空"));
        let empty = parse(r#"{"tasks": [{"title": "交初稿", "due": "", "description": null, "ref": " "}]}"#, &shown).unwrap();
        assert!(empty[0].due.is_none() && empty[0].description.is_none() && empty[0].name.is_none());
        assert!(parse(r#"{"tasks": [{"title": "a", "note": "b"}]}"#, &shown).is_err());
        assert_eq!(parse(r#"{"tasks": [{"title": "a", "folder": "/x"}]}"#, &shown).err().unwrap(), "模型返回的结果指定了文件夹");
        assert_eq!(parse(r#"{"tasks": [{"title": "a", "before": [7]}]}"#, &shown).err().unwrap(), "结果引用了不在这个文件夹里的事项 #7");
    }
}
