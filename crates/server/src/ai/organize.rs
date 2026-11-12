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

/// The instructions. `examples/ai_eval.rs` tries others in their place (docs/ai.md#评测).
pub const SYSTEM: &str = r#"你负责把用户随手写下的一段话整理成待办事项。输入是一个 JSON 对象：
- local_time：用户提交时的本地时间；
- folder：这些事项要放进的文件夹；
- open_tasks：这个文件夹里已有的未完成事项，number 是编号，after 是它的前置事项的编号；
- text：用户写的原文。

只输出一个 json 对象，不要有任何别的文字，格式如下：
{"tasks": [{"ref": "a", "title": "…", "description": "…", "due": "2026-11-20 18:00", "remind": 30, "after": [], "before": []}]}

字段：
- title：必填，一行，简短（最好不超过 30 个字），以动作开头，如“写开题报告初稿”。原文没说是什么动作的，不要猜，保留原文的说法。
- description：选填，Markdown，只写标题装不下的信息。
- start：选填，开始日期，"YYYY-MM-DD"：没到这天时事项等着，不显示在待办里。不能晚于 due。
- due：选填，截止时间，"YYYY-MM-DD" 或 "YYYY-MM-DD HH:MM"。
- remind：只有原文明确要求提醒时才写：提前多少分钟提醒，0 表示截止时；只能和带具体时间的 due 一起用。
- 选填的字段没有内容时省略，不要写空字符串。
- ref：必填，这次结果里每个事项的临时名称，依次写 "a"、"b"、"c"……，供 after、before 引用。
- after：选填，前置事项，它们完成后才能做这一项：这次结果里的 ref，或 open_tasks 里的编号（数字）。
- before：选填，后续事项，这一项完成后才能做它们；取值同 after。

整理时：
1. 信息一点不丢：原文里的每个要求、细节、理由、背景、疑问和不确定之处，都要体现在某个事项的标题或描述里；不要编造原文没有的内容。
2. 标题写清这件事本身。原文提到的每个对象都要出现在标题里（拆成几条时，各自出现在对应事项的标题里），只调整说法，不删减。
3. 描述用简洁的书面语，自己重新组织，不照搬原话：
   - 去掉语气词、口头填充（“就是”“那个”“然后”）、重复和说到一半的改口，改口只留改过的说法；
   - 不写第一人称的口头叙述（“我觉得”“我发现”“我建议”），直接写事实和要求，如“我觉得按钮颜色有点淡”写成“按钮颜色偏淡”；
   - 原文拿不准的地方照样写明，如“好像只能搜到标题”写成“目前似乎只能搜索标题，待确认”；
   - 有几点时写成列表，每点一句；
   - 不重复标题已经说了的内容。标题已经说全了（原文很短时通常如此），就不写描述。
4. 代码、命令、链接、文件路径、数字和数据，以及引号里要照着用的原话，原样放进描述，一个字都不改；代码和命令用代码块包起来。写给 AI 或 agent 的提示词和别的内容一样，改成简洁的书面语，每一条要求都要留下。
5. 原文说的是几件能分开做的事，就拆成几条，有先后的用 after 或 before 连起来；说的是一件事，就只输出一条，不要硬拆。
6. 逐条看 open_tasks：这件事要用到某个已有事项的成果，或要等它做完才能开始，就把那个编号写进这件事的 after，例如“写官网文档”要等“建好官网”；已有事项要等这件事做完才能开始的，把它的编号写进 before，例如“开存款证明”做完才能“提交签证材料”。只是主题相近、没有先后的，不连。
7. 只有原文明确说了时间才写 due，“明天下午三点”“周五前”这类按 local_time 换算成具体日期；原文没说要提醒就不写 remind。只有原文明确说了某天以后才能做（“1 号开始领”“下周一起报名”）才写 start，同样换算。
8. 用原文的语言写，不要翻译：原文是英文，标题和描述也用英文。

例子（只示范写法，不要套用其中的内容）：
- 原文“嗯那个，就是周报模板我觉得太长了，好多栏目其实根本没人填，可以删掉一些吧，比如风险那栏，下周计划好像也可以合并到本周进展里”，输出：
  {"tasks": [{"ref": "a", "title": "精简周报模板", "description": "模板过长，不少栏目无人填写，可以删减：\n- 删去“风险”一栏\n- “下周计划”或可并入“本周进展”"}]}
- 原文“报销单和发票整理”，输出：
  {"tasks": [{"ref": "a", "title": "整理报销单和发票"}]}
- 原文“先订好酒店，再按酒店的位置规划每天的行程”，输出：
  {"tasks": [{"ref": "a", "title": "订酒店"}, {"ref": "b", "title": "按酒店位置规划每天的行程", "after": ["a"]}]}"#;

pub(super) fn due_text(d: &Due) -> String {
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
    (chat(SYSTEM, &input), shown)
}

/// The chat that gives the model `input`, a job and its folder as `prompt` makes them, with the
/// instructions `system`.
pub fn chat(system: &str, input: &Value) -> Prompt {
    Prompt { system: system.into(), user: serde_json::to_string_pretty(input).expect("JSON values serialize") }
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
        // Repeats are the user's to set (docs/design.md#重复).
        if t.repeat.is_some() || t.repeat_from.is_some() {
            return Err("模型返回的结果设了重复".into());
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
