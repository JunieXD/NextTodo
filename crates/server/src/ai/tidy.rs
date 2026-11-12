//! Tidying a folder: what a job asks the model, and reading back what it suggests doing there, which
//! the user then takes or leaves part by part. See docs/ai.md#整理文件夹.

use std::collections::{HashMap, HashSet};

use nexttodo_core::board::{Board, Snapshot};
use nexttodo_core::dag::find_path;
use nexttodo_core::model::{
    AiJob, Date, Description, FolderId, NewTask, Period, Reorder, Repeat, RepeatFrom, Suggestion, Task, TaskDeletion, TaskEdit, TaskId,
    Title, Was,
};
use nexttodo_core::ops::MAX_OPS;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::llm::Prompt;
use super::organize::{chat, due_text, unwrap};
use crate::error::from_value;
use crate::tools::args::{DueText, EditArgs, PlanRef, RepeatText};
use crate::tools::{Edit, edit_fields, new_due, number};

/// How many of the folder's open tasks the model is shown, and of its done ones, the latest.
const SHOWN: usize = 50;
const DONE_SHOWN: usize = 20;
/// How many of the folders inside it, to move tasks into.
const FOLDERS_SHOWN: usize = 200;
/// How much of the open tasks' descriptions, in characters: tasks past it go without theirs.
const DESCRIBED: usize = 30_000;

/// The instructions. `examples/ai_eval.rs` tries others in their place (docs/ai.md#评测).
pub const SYSTEM: &str = r#"你负责按用户的要求整理一个文件夹里的待办事项：提出要做的修改，由用户逐条确认后再执行。输入是一个 JSON 对象：
- local_time：用户提交时的本地时间；
- folder：这个文件夹；
- request：用户这次的要求；
- subfolders：这个文件夹下的子文件夹，可以把事项移进去；
- open_tasks：文件夹里未完成的事项，按现在的顺序。number 是编号；requires 是它已有的前置事项的编号，那些事项要先做完才能做它；due 是截止时间，remind 是提前多少分钟提醒，repeat 是重复，写成间隔数加单位 d（天）、w（周）、m（月）或 y（年，就是 12 个月），如 1d 每天、2w 每两周、1m 每月、3m 每三个月、1y 每年；按周重复在某几天的，冒号后写周几（1 是周一，7 是周日），如 1w:1,2,3,4,5 每个工作日、1w:1,3,5 每周一三五、2w:2,4 每两周的周二和周四；按月重复在某几号的，冒号后写几号（1–31，没有这天的月份取最后一天），如 1m:1,15 每月 1 号和 15 号、1m:31 每月最后一天；repeat_from 为 done 表示下一次从完成那天算；description_omitted 为 true 的事项描述太长，没有发来；
- done_tasks：最近完成的事项，只有编号和标题。

只输出一个 json 对象，不要有任何别的文字，格式如下：
{"suggestions": [
  {"number": 12, "title": "…", "description": "…", "due": "2026-11-20 18:00", "requires": [3], "reason": "…"},
  {"number": 15, "delete": true, "reason": "…"},
  {"ref": "a", "title": "…", "required_by": [12], "reason": "…"},
  {"order": [14, 12, 13], "reason": "…"}
]}

每条建议是下面四种之一，都要写 reason，用一句话说明做什么、为什么。requires、required_by、drop_requires、clear、order 都是列表，只有一项也写成列表，如 [3]：
1. 修改已有的事项：写 number，再只写要改的字段，不改的省略，不要写空字符串或 null。同一个事项的几处修改写在同一条里，每个事项最多出现一次：
   - title：新标题，一行，简短（最好不超过 30 个字），以动作开头；原标题没说是什么动作的，不要为了以动作开头而猜一个动作或删词。
   - description：新描述，是改好后的完整内容，不是修改说明；Markdown，步骤可以写成清单“- [ ] ”。
   - due：截止时间，"YYYY-MM-DD" 或 "YYYY-MM-DD HH:MM"。原来有几点几分、这次只改日期的，带上原来的几点几分；原来和 request 都没说几点的，不要加；原来的提醒和重复保留，新的截止时间不带具体时间的，提醒去掉。
   - remind：提前多少分钟提醒，0 表示到截止时间时提醒；要有带具体时间的截止时间。“不用提醒了”是去掉提醒，写 clear: ["remind"]，不是写 0。
   - repeat、repeat_from：重复，写法同输入；要有截止时间。按天数说的照天数写（每 90 天是 90d，不是 3m）；在一周里某几天的写周几，“每个工作日”是 1w:1,2,3,4,5，不是 1d（1d 周末也会出现）。
   - clear：要清除的设置，是一个列表，可以包含 "due"、"remind"、"repeat"，如 "clear": ["due"]；清除截止时间时，提醒和重复一起清除。去掉前置用 drop_requires，不用 clear。
   - done：true 标为完成（重复的事项会顺延到下一次；request 说这件事整个做完了、不再重复的，如书读完了，同时在 clear 里写 "repeat"），false 重新打开 done_tasks 里的事项。done_tasks 里的事项可以删除；要改别的，先重新打开。
   - folder：移进 subfolders 里的哪个文件夹，写它的完整路径。subfolders 里没有的文件夹移不进去，这样的移动不要提。
   - requires：这一项要新加的前置，也就是要先做完、才能做这一项的事项：事项的编号，或新建事项的 ref；已有的不用写。
   - drop_requires：要去掉的这一项已有的前置，写编号。
2. 删除已有的事项：写 number 和 "delete": true。
3. 新建事项，放进这个文件夹：写 ref，依次是 "a"、"b"、"c"……，和 title；选填 description、due、remind、repeat、repeat_from，以及 requires（要先做完、才能做它的事项）和 required_by（要等它做完才能做的事项），写事项的编号或新建事项的 ref。
4. 调整顺序：order 是 open_tasks 里事项的编号，按新的顺序排；只写要调整的那些，它们在原来占着的位置之间重新排列。order 只改列表里的位置，前置总排在后续前面。request 要的顺序和已有的先后冲突时（如要把“上线”排到最前，而它要等“测试”），保留先后，只排没有冲突的那些；除非 request 明说要改先后，不要为了排序去掉或反转前置。

整理时：
1. 只做 request 说了的事，没说的一律不做。例如 request 是“把交报告推迟到周五”，就只改“交报告”的 due：别的事项不列出，也不要顺手改标题，或给“写报告”这类相关的事项补先后。request 要求把标题改清楚、描述改成书面语、补上先后时，才做这些整理；这时标题或描述里写明了要在什么时候之前完成、事项又还没有截止时间的，加上 due；只是提到某个时间（如“周四下午有空”）不算截止时间，也不要自己编出原文没有的几点几分。新建、删除、完成、重新打开、移动、改已有的截止时间、设置或改提醒和重复、去掉前置、调整顺序，都只在 request 要求时才做；“每月 5 号前”这类说法写在描述里就行，request 没要求就不设重复。
2. request 说的是一类事项（如“写文档的”“买东西的”“今天到期的”“复习相关的”）时，逐条对照 open_tasks，属于这一类的都包括进来，不要只挑标题里有那个词的；不属于的不动。“今天到期”“过期”按 local_time 和事项的 due 判断，没有 due 的事项不算。
3. 改写不能丢信息：原标题和描述里的每一条要求、细节、理由、背景和疑问，都要留在新标题或新描述里，不要概括掉；不要编造原文没有的内容。改了标题时，原标题提到的每个对象都要留在新标题里，例如“报销单和发票整理”改成“整理报销单和发票”，不能只剩“整理报销单”；放不下的细节写进描述。
4. 描述常是语音输入的口语。改写成简洁的书面语：去掉语气词、重复、说到一半的改口和“我觉得”“就是说”这类口头填充，但不能因此丢掉信息：改口只留改过的说法，改口的理由（如“周三有课”）要留下。
5. 代码、命令、链接、文件路径、数字和数据，以及引号里要照着用的原话，原样保留，一个字都不改。写给 AI 或 agent 的提示词和别的内容一样，改成简洁的书面语，每一条要求都要留下。
6. 一个事项要用到另一个事项的成果，或要等它做完才能开始，就把那个事项写进它的 requires，例如“写官网文档”要等“建好官网”。request 要求补上先后时，逐对检查 open_tasks：一项不做完、另一项就没法开始的，都要连上；标题或描述里写明了的先后（如“请柬要等酒店定了才能发”），拆分、新建时也要连上。只是主题相近，或者“可能”用得上的，不连。不要连成环。
   - requires 写在后做的那一项上，列出先做的：{"number": 8, "requires": [7]} 表示先做 7、再做 8。
   - request 说的是做事的先后（“按这个顺序做”“先 A、再 B、然后 C”）时，用 requires，相邻的每一对都要连上：B 的 requires 有 A，C 的 requires 有 B。说的是列表里的位置（“把列表顺序调成……”“排一下顺序”）时，用 order。
   - 要反过来改成先做 8、再做 7，写 {"number": 8, "drop_requires": [7]} 和 {"number": 7, "requires": [8]}。
   - 要在已有的 8 前面加一步，新建 {"ref": "a", "title": "…", "required_by": [8]}；加在 8 后面，写 "requires": [8]。
7. “周五前”“下周一”这类时间，按 local_time 换算成具体日期；一周从周一开始，“这周五”是本周的周五，“下周五”是下一周的周五。“周五前”“月底前”这类只说了哪天的，只写日期，不要加几点。
8. 已经做完的事项标为完成（done），不要删除；删除只用于不再需要做的事项。合并重复的事项时，留下一项，把另一项的信息并进它的标题或描述，再删除另一项；删掉的那项有前置或后续的，转到留下的那项上（后续写在后续自己的 requires 里），先后不能因合并而丢。
9. description_omitted 为 true 的事项和 done_tasks 里的事项，不要写 title 和 description：没看到描述，改标题可能丢掉信息。
10. 已经清楚的事项不要列出，不要为改而改：标题已经以动作开头、意思清楚的（如“交周报”“买牛奶”），不要换个同义的说法，如“交”改“提交”、“查”改“查阅”、“写”改“撰写”；没有要做的修改时输出 {"suggestions": []}。
11. 用原文的语言写。
12. 输出前逐条检查：
   - 每条建议都是 request 要求的；不是的删掉。
   - 把建议应用到事项上（没写的字段沿用原值），原标题和原描述的每一处都要能在改好后的标题或描述里找到，找不到的补上。
   - requires 写在后做的那一项上，里面是先做的事项；写反的改过来。"#;

/// What the model was shown: the data as it was then, the folder, and which of its tasks.
pub struct Shown {
    snap: Snapshot,
    /// The day the board is as of: the job's in UTC, since tidying reads no task's status.
    day: Date,
    folder: Option<FolderId>,
    tasks: HashMap<i64, Seen>,
}

#[derive(Clone, Copy, PartialEq)]
enum Seen {
    /// Open, and whether with its description: only then may the model retitle it or rewrite that.
    Open {
        described: bool,
    },
    Done,
}

/// The chat for `job`, a tidying of its folder in `snap`, and what was shown in it.
pub fn prompt(job: &AiJob, snap: Snapshot) -> (Prompt, Shown) {
    let day = Date::utc(job.created_at);
    let board = Board::new(&snap, day.clone());
    let all = board.tasks_in(job.folder_id);
    let open: Vec<&Task> = all.iter().copied().filter(|t| t.done_at.is_none()).take(SHOWN).collect();
    // Listed most recently done first.
    let done: Vec<&Task> = all.iter().copied().filter(|t| t.done_at.is_some()).take(DONE_SHOWN).collect();
    let numbers: HashSet<i64> = open.iter().map(|t| t.number).collect();
    let mut budget = DESCRIBED;
    let mut tasks = HashMap::new();
    let open_tasks: Vec<Value> = open
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
                if let Some(m) = d.at.as_ref().and_then(|at| at.remind_before) {
                    v["remind"] = m.into();
                }
                if let Some(r) = d.repeat {
                    v["repeat"] = repeat_text(r).into();
                    if r.from == RepeatFrom::Done {
                        v["repeat_from"] = "done".into();
                    }
                }
            }
            let after: Vec<i64> = board.before(t).iter().map(|b| b.number).filter(|n| numbers.contains(n)).collect();
            if !after.is_empty() {
                v["requires"] = after.into();
            }
            tasks.insert(t.number, Seen::Open { described });
            v
        })
        .collect();
    let done_tasks: Vec<Value> = done
        .iter()
        .map(|t| {
            tasks.insert(t.number, Seen::Done);
            json!({ "number": t.number, "title": t.title.as_str() })
        })
        .collect();
    let subfolders: Vec<String> = board
        .folder_tree()
        .into_iter()
        .filter(|&f| f != job.folder_id && board.in_subtree(f, job.folder_id))
        .take(FOLDERS_SHOWN)
        .map(|f| board.path(f))
        .collect();
    let input = json!({
        "local_time": job.local_time.as_str(),
        "folder": board.path(job.folder_id),
        "request": job.text.as_str(),
        "subfolders": subfolders,
        "open_tasks": open_tasks,
        "done_tasks": done_tasks,
    });
    (chat(SYSTEM, &input), Shown { snap, day, folder: job.folder_id, tasks })
}

/// `1d`, `2w`, `3m`, `1w:1,3,5`, `1m:1,15`, as the tools take a repeat.
pub fn repeat_text(r: Repeat) -> String {
    let (unit, days) = match r.period {
        Period::Day => ('d', None),
        Period::Week(days) => ('w', days.map(Vec::from)),
        Period::Month(days) => ('m', days.map(Vec::from)),
    };
    let days = days.map(|d| format!(":{}", d.iter().map(u8::to_string).collect::<Vec<_>>().join(",")));
    format!("{}{unit}{}", r.every.get(), days.unwrap_or_default())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    suggestions: Vec<Suggested>,
}

/// A suggestion as the model writes it: a change to the task `number`, its deletion, a new task, or
/// an `order`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Suggested {
    number: Option<i64>,
    #[serde(rename = "ref")]
    name: Option<String>,
    order: Option<Vec<i64>>,
    #[serde(default)]
    delete: bool,
    title: Option<Title>,
    description: Option<Description>,
    due: Option<DueText>,
    remind: Option<u32>,
    repeat: Option<RepeatText>,
    repeat_from: Option<RepeatFrom>,
    #[serde(default)]
    clear: Vec<Setting>,
    done: Option<bool>,
    folder: Option<String>,
    #[serde(default, rename = "requires")]
    after: Vec<PlanRef>,
    #[serde(default, rename = "required_by")]
    before: Vec<PlanRef>,
    #[serde(default, rename = "drop_requires")]
    unlink: Vec<i64>,
    /// Models leave it out at times.
    #[serde(default)]
    reason: String,
}

impl Suggested {
    /// Whether it says to do anything: models write some for a task with nothing to do in it.
    fn says(&self) -> bool {
        self.delete
            || self.title.is_some()
            || self.description.is_some()
            || self.due.is_some()
            || self.remind.is_some()
            || self.repeat.is_some()
            || self.repeat_from.is_some()
            || !self.clear.is_empty()
            || self.done.is_some()
            || self.folder.is_some()
            || !self.after.is_empty()
            || !self.before.is_empty()
            || !self.unlink.is_empty()
    }

    /// The one suggestion `self` and `later`, for the same task, make: what both change, the later
    /// one's way where they differ.
    fn merge(&mut self, later: Suggested) {
        if !later.says() {
            return;
        }
        self.reason = match (self.says(), self.reason.is_empty(), later.reason.is_empty()) {
            (true, false, false) => format!("{}；{}", self.reason, later.reason),
            (_, _, false) => later.reason,
            _ => std::mem::take(&mut self.reason),
        };
        self.delete |= later.delete;
        // A setting one clears and the other sets is as the later one says.
        self.clear.retain(|c| match c {
            Setting::Due => later.due.is_none(),
            Setting::Remind => later.remind.is_none(),
            Setting::Repeat => later.repeat.is_none(),
        });
        for c in &later.clear {
            match c {
                Setting::Due => self.due = None,
                Setting::Remind => self.remind = None,
                Setting::Repeat => self.repeat = None,
            }
        }
        self.title = later.title.or(self.title.take());
        self.description = later.description.or(self.description.take());
        self.due = later.due.or(self.due.take());
        self.remind = later.remind.or(self.remind);
        self.repeat = later.repeat.or(self.repeat);
        self.repeat_from = later.repeat_from.or(self.repeat_from);
        self.clear.extend(later.clear);
        self.done = later.done.or(self.done);
        self.folder = later.folder.or(self.folder.take());
        self.after.extend(later.after);
        self.before.extend(later.before);
        self.unlink.extend(later.unlink);
    }
}

/// What `clear` clears: as `null` does for `edit`, which models write for what they leave alone.
#[derive(Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Setting {
    Due,
    Remind,
    Repeat,
}

/// What the model suggests, for tasks it was shown, leaving out what would change nothing. Checked
/// as the change taking all of it would be: links that close no cycle, few enough operations.
pub fn parse(answer: &str, shown: &Shown) -> Result<Vec<Suggestion>, String> {
    let mut v: Value = serde_json::from_str(unwrap(answer)).map_err(|_| "模型返回的不是合法的 JSON".to_owned())?;
    // An empty field is one the model has nothing for, as in organizing.
    for s in v["suggestions"].as_array_mut().into_iter().flatten() {
        if let Some(fields) = s.as_object_mut() {
            fields.retain(|k, f| k == "reason" || !(f.is_null() || f.as_str().is_some_and(|s| s.trim().is_empty())));
        }
    }
    let Answer { suggestions: all } = from_value(v).map_err(|e| format!("模型返回的结果不符合格式：{}", e.describe()))?;
    // Models split what they suggest for a task at times, however they are told.
    let mut suggestions: Vec<Suggested> = Vec::new();
    for s in all {
        match suggestions.iter_mut().find(|x| x.order.is_none() && s.order.is_none() && x.number.is_some() && x.number == s.number) {
            Some(first) => first.merge(s),
            None => suggestions.push(s),
        }
    }
    let board = Board::new(&shown.snap, shown.day.clone());
    let task = |n: i64| -> Result<(&Task, Seen), String> {
        let seen = *shown.tasks.get(&n).ok_or_else(|| format!("结果引用了不在这个文件夹里的事项 #{n}"))?;
        Ok((board.task_by_number(n).expect("shown tasks are in the snapshot"), seen))
    };
    // New tasks get their ids first: suggestions before them may refer to them.
    let mut refs = HashMap::new();
    let mut new = HashMap::new();
    let ids: Vec<Option<TaskId>> =
        suggestions.iter().map(|s| (s.number.is_none() && s.order.is_none()).then(|| TaskId(Uuid::now_v7()))).collect();
    for (s, id) in suggestions.iter().zip(&ids) {
        let (Some(id), Some(name)) = (id, &s.name) else { continue };
        if number(name).is_some() {
            return Err(format!("新事项的 ref“{name}”会被当成事项编号"));
        }
        if refs.insert(name.clone(), *id).is_some() {
            return Err(format!("结果里有多个新事项的 ref 都是“{name}”"));
        }
    }
    let resolve = |r: &PlanRef| -> Result<TaskId, String> {
        match r {
            PlanRef::Number(n) => Ok(task(*n)?.0.id),
            PlanRef::Name(s) => match number(s) {
                Some(n) => Ok(task(n)?.0.id),
                None => refs.get(s).copied().ok_or_else(|| format!("结果里没有 ref 为“{s}”的新事项")),
            },
        }
    };

    let edges: Vec<(TaskId, TaskId)> = shown.snap.dependencies.iter().map(|d| (d.before, d.after)).collect();
    let mut out = Vec::new();
    let mut order = None;
    let mut deleted = HashSet::new();
    for (s, id) in suggestions.into_iter().zip(ids) {
        if let Some(numbers) = s.order {
            if s.number.is_some() || s.name.is_some() {
                return Err("调整顺序的建议不能同时写 number 或 ref".into());
            }
            if order.replace((numbers, s.reason)).is_some() {
                return Err("结果里调整顺序的建议不止一条".into());
            }
            continue;
        }
        let Some(n) = s.number else {
            let id = id.expect("new tasks have ids");
            let title = s.title.ok_or("新事项缺少标题")?;
            if s.delete || s.done.is_some() || s.folder.is_some() || !s.clear.is_empty() || !s.unlink.is_empty() {
                return Err(format!(
                    "新事项“{}”只能写 title、description、due、remind、repeat、repeat_from、requires 和 required_by",
                    title.as_str()
                ));
            }
            let due =
                new_due(s.due, s.remind, s.repeat, s.repeat_from).map_err(|e| format!("新事项“{}”：{}", title.as_str(), e.describe()))?;
            let after = distinct(s.after.iter().map(&resolve))?;
            let before = distinct(s.before.iter().map(&resolve))?;
            new.insert(id, title.clone());
            out.push(Suggestion::Create(NewTask { create: id, title, description: s.description, due, after, before, reason: s.reason }));
            continue;
        };
        if s.name.is_some() {
            return Err(format!("修改事项 #{n} 的建议不能写 ref"));
        }
        if !s.before.is_empty() {
            return Err(format!("修改事项 #{n} 的建议不能写 required_by，后续要写在后续事项自己的 requires 里"));
        }
        let (t, state) = task(n)?;
        let was = Was { title: t.title.clone(), description: t.description.clone(), due: t.due.clone() };
        if s.delete {
            deleted.insert(t.id);
            out.push(Suggestion::Delete(TaskDeletion { delete: t.id, reason: s.reason, was }));
            continue;
        }
        // Not having seen the description, it would lose what the title held that this lacks.
        let described = state == Seen::Open { described: true };
        let title = s.title.filter(|_| described);
        let description = s.description.filter(|_| described);
        let fields = title.is_some()
            || description.is_some()
            || s.due.is_some()
            || s.remind.is_some()
            || s.repeat.is_some()
            || s.repeat_from.is_some()
            || !s.clear.is_empty();
        let Edit { title, description, due, .. } = if fields {
            let clear = |x| s.clear.contains(&x);
            let args = EditArgs {
                task: n,
                title,
                description,
                replace: Vec::new(),
                append: None,
                start: None,
                due: if clear(Setting::Due) { Some(None) } else { s.due.map(Some) },
                remind: if clear(Setting::Remind) { Some(None) } else { s.remind.map(Some) },
                repeat: if clear(Setting::Repeat) { Some(None) } else { s.repeat.map(Some) },
                repeat_from: s.repeat_from,
            };
            edit_fields(&board, args).map_err(|e| format!("事项 #{n}：{}", e.describe()))?
        } else {
            Edit { id: t.id, title: None, description: None, start: None, due: None }
        };
        let folder = match s.folder.as_deref().map(|p| (p, board.find_folder(p))) {
            None => None,
            Some((_, Some(f))) if f == t.folder_id => None,
            Some((_, Some(Some(f)))) if board.in_subtree(Some(f), shown.folder) => Some(f),
            Some((p, _)) => return Err(format!("建议把 #{n} 移到 {p}，但它不是这个文件夹下的子文件夹")),
        };
        let after: Vec<TaskId> = distinct(s.after.iter().map(&resolve))?.into_iter().filter(|&b| !edges.contains(&(b, t.id))).collect();
        let unlink: Vec<TaskId> =
            distinct(s.unlink.iter().map(|&m| task(m).map(|(b, _)| b.id)))?.into_iter().filter(|&b| edges.contains(&(b, t.id))).collect();
        let edit = TaskEdit {
            task: t.id,
            title: title.filter(|x| *x != t.title),
            description: description.filter(|x| *x != t.description),
            due: due.filter(|d| *d != t.due),
            done: s.done.filter(|&d| d != t.done_at.is_some()),
            folder,
            after,
            unlink,
            reason: s.reason,
            was,
        };
        if state == Seen::Done && edit.done.is_none() && changes(&edit) {
            return Err(format!("事项 #{n} 已完成，要先重新打开才能修改"));
        }
        out.push(Suggestion::Edit(edit));
    }

    // A task deleted goes with its dependencies: those suggested for it are of no use.
    for s in &mut out {
        match s {
            Suggestion::Edit(e) => {
                e.after.retain(|b| !deleted.contains(b));
                e.unlink.retain(|b| !deleted.contains(b));
            }
            Suggestion::Create(c) => {
                c.after.retain(|b| !deleted.contains(b));
                c.before.retain(|a| !deleted.contains(a));
            }
            Suggestion::Delete(_) | Suggestion::Order(_) => {}
        }
    }
    out.retain(|s| !matches!(s, Suggestion::Edit(e) if !changes(e) && e.done.is_none()));
    let mut links = Vec::new();
    let mut unlinks = HashSet::new();
    for s in &out {
        match s {
            Suggestion::Edit(e) => {
                links.extend(e.after.iter().map(|&b| (b, e.task)));
                unlinks.extend(e.unlink.iter().map(|&b| (b, e.task)));
            }
            Suggestion::Create(c) => links.extend(c.after.iter().map(|&b| (b, c.create)).chain(c.before.iter().map(|&a| (c.create, a)))),
            Suggestion::Delete(_) | Suggestion::Order(_) => {}
        }
    }

    let name = |id: TaskId| {
        new.get(&id).map_or_else(|| format!("#{}", board.task(id).expect("linked tasks are shown").number), |t| format!("“{}”", t.as_str()))
    };
    let mut edges: Vec<(TaskId, TaskId)> =
        edges.into_iter().filter(|e| !unlinks.contains(e) && !deleted.contains(&e.0) && !deleted.contains(&e.1)).collect();
    for (b, a) in links {
        if edges.contains(&(b, a)) {
            continue;
        }
        if find_path(&edges, a, b).is_some() {
            return Err(format!("建议把 {} 设为 {} 的前置，但这样依赖会成环", name(b), name(a)));
        }
        edges.push((b, a));
    }

    if let Some((numbers, reason)) = order {
        let mut ids = Vec::new();
        for n in numbers {
            let (t, state) = task(n)?;
            if state == Seen::Done {
                return Err(format!("调整顺序只能排未完成的事项，#{n} 已完成"));
            }
            if ids.contains(&t.id) {
                return Err(format!("调整顺序里事项 #{n} 出现了不止一次"));
            }
            if !deleted.contains(&t.id) {
                ids.push(t.id);
            }
        }
        let now: Vec<TaskId> = board.tasks_in(shown.folder).iter().map(|t| t.id).filter(|id| ids.contains(id)).collect();
        if ids.len() > 1 && ids != now {
            out.push(Suggestion::Order(Reorder { order: ids, reason }));
        }
    }

    // Taking all of it is one change, which deletes the job too.
    let ops: usize = out
        .iter()
        .map(|s| match s {
            Suggestion::Edit(e) => {
                usize::from(e.title.is_some() || e.description.is_some() || e.due.is_some() || e.done.is_some() || e.folder.is_some())
                    + e.after.len()
                    + e.unlink.len()
            }
            Suggestion::Delete(_) => 1,
            Suggestion::Create(c) => 1 + c.after.len() + c.before.len(),
            Suggestion::Order(o) => o.order.len(),
        })
        .sum();
    if ops + 1 > MAX_OPS {
        return Err("建议的修改太多，一次应用不了，请缩小范围后重试".into());
    }
    Ok(out)
}

/// Whether `e` changes anything but whether the task is done.
fn changes(e: &TaskEdit) -> bool {
    e.title.is_some() || e.description.is_some() || e.due.is_some() || e.folder.is_some() || !e.after.is_empty() || !e.unlink.is_empty()
}

/// The tasks `refs` find, each once.
fn distinct(refs: impl Iterator<Item = Result<TaskId, String>>) -> Result<Vec<TaskId>, String> {
    let mut out = Vec::new();
    for id in refs {
        let id = id?;
        if !out.contains(&id) {
            out.push(id);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexttodo_core::model::Dependency;

    fn id(n: i64) -> String {
        format!("00000000-0000-7000-8000-{n:012}")
    }

    fn task(n: i64, title: &str, extra: Value) -> Task {
        let mut v = json!({
            "id": id(n), "number": n, "folder_id": id(100), "title": title, "description": "", "done_at": null, "due": null,
            "sort_key": format!("a{n:02}1"), "created_at": 0, "updated_at": 0, "created_via": null, "updated_via": null,
        });
        v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        serde_json::from_value(v).unwrap()
    }

    fn snap() -> Snapshot {
        let folder = |n: i64, name: &str, parent: Option<i64>| {
            serde_json::from_value(
                json!({ "id": id(n), "parent_id": parent.map(id), "name": name, "sort_key": "a0", "created_at": 0, "updated_at": 0 }),
            )
            .unwrap()
        };
        // The last two are too long together: the second goes without its description.
        let long = "字".repeat(18_000);
        let tasks = vec![
            task(1, "官网", json!({ "description": "就是说要做一个官网" })),
            task(2, "写官网文档", json!({ "due": { "date": "2026-11-20", "time": "18:00", "remind_before": 30 } })),
            task(3, "长的", json!({ "description": long })),
            task(
                4,
                "也长",
                json!({ "description": long, "due": { "date": "2026-11-09", "repeat": { "every": 1, "unit": "week", "from": "done" } } }),
            ),
            task(5, "订机票", json!({ "done_at": 1 })),
            task(6, "别处的", json!({ "folder_id": null })),
        ];
        let dependencies = vec![Dependency { before: tasks[0].id, after: tasks[1].id, created_at: 0 }];
        let folders = vec![folder(100, "官网", None), folder(101, "文档", Some(100)), folder(102, "别处", None)];
        Snapshot { folders, tasks, dependencies, ..Default::default() }
    }

    fn job() -> AiJob {
        serde_json::from_value(json!({
            "id": "00000000-0000-7000-8000-000000000099", "folder_id": id(100), "kind": "tidy", "text": "整理一下",
            "local_time": "2026-11-07 10:00 星期六", "state": "running", "error": null, "created_at": 0, "updated_at": 0,
        }))
        .unwrap()
    }

    fn tid(n: i64) -> TaskId {
        TaskId(id(n).parse().unwrap())
    }

    #[test]
    fn shows_the_folder() {
        let (prompt, shown) = prompt(&job(), snap());
        let input: Value = serde_json::from_str(&prompt.user).unwrap();
        assert_eq!(input["folder"], "/官网");
        assert_eq!(input["request"], "整理一下");
        assert_eq!(input["subfolders"], json!(["/官网/文档"]));
        assert_eq!(
            input["open_tasks"][1],
            json!({ "number": 2, "title": "写官网文档", "due": "2026-11-20 18:00", "remind": 30, "requires": [1] })
        );
        assert_eq!(input["open_tasks"][2]["description"].as_str().unwrap().len(), 18_000 * 3);
        assert_eq!(
            input["open_tasks"][3],
            json!({ "number": 4, "title": "也长", "description_omitted": true, "due": "2026-11-09", "repeat": "1w", "repeat_from": "done" })
        );
        assert_eq!(input["done_tasks"], json!([{ "number": 5, "title": "订机票" }]));
        assert_eq!(input["open_tasks"].as_array().unwrap().len(), 4);
        assert!(shown.tasks[&3] == Seen::Open { described: true } && shown.tasks[&4] == Seen::Open { described: false });
    }

    #[test]
    fn reads_each_kind() {
        let (_, shown) = prompt(&job(), snap());
        let ok = r#"{"suggestions": [
            {"number": 1, "title": "建好官网", "description": "做一个官网。", "due": "2026-11-20", "requires": ["a"], "reason": "标题以动作开头"},
            {"number": 2, "due": "2026-11-21 09:00", "folder": "/官网/文档", "requires": [1], "drop_requires": [4], "reason": "推迟一天"},
            {"number": 4, "title": "短", "done": true, "clear": ["repeat"], "reason": "描述没发来"},
            {"number": 3, "delete": true, "title": "删了", "reason": "重复了"},
            {"ref": "a", "title": "选域名", "due": "2026-11-15 10:00", "remind": 60, "required_by": [2], "reason": "要先有域名"},
            {"number": 5, "done": false, "requires": [1], "reason": "还没订"},
            {"order": [2, 1], "reason": "反过来"}
        ]}"#;
        let s = parse(ok, &shown).unwrap();
        let [
            Suggestion::Edit(a),
            Suggestion::Edit(b),
            Suggestion::Edit(c),
            Suggestion::Delete(d),
            Suggestion::Create(e),
            Suggestion::Edit(f),
            Suggestion::Order(g),
        ] = s.as_slice()
        else {
            panic!("{s:?}")
        };
        assert_eq!(a.title.as_ref().unwrap().as_str(), "建好官网");
        assert_eq!(a.after, [e.create]);
        assert_eq!(a.was.title.as_str(), "官网");
        // Moved on, the reminder stays; an existing link is not added again, nor a missing one removed.
        let due = b.due.clone().flatten().unwrap();
        assert_eq!((due.date.as_str(), due.at.unwrap().remind_before), ("2026-11-21", Some(30)));
        assert_eq!(b.folder, Some(FolderId(id(101).parse().unwrap())));
        assert!(b.after.is_empty() && b.unlink.is_empty());
        // Not having seen the description, no new title.
        assert!(c.title.is_none() && c.done == Some(true));
        assert_eq!(c.due.clone().flatten().unwrap().repeat, None);
        assert_eq!(d.delete, tid(3));
        assert_eq!(e.before, [tid(2)]);
        assert_eq!(e.due.as_ref().unwrap().at.as_ref().unwrap().remind_before, Some(60));
        assert!(f.done == Some(false) && f.after == [tid(1)]);
        assert_eq!(g.order, [tid(2), tid(1)]);
        // Given twice, a task's suggestion is one, the later's way where they differ.
        let twice = r#"{"suggestions": [
            {"number": 1, "title": "a", "requires": [3], "clear": ["due"], "reason": "一"}, {"number": 1, "drop_requires": [], "reason": "空"},
            {"number": 1, "title": "b", "description": "c", "due": "2026-12-01", "reason": "二"}
        ]}"#;
        let merged = parse(twice, &shown).unwrap();
        let [Suggestion::Edit(e)] = merged.as_slice() else { panic!("{merged:?}") };
        assert_eq!((e.title.as_ref().unwrap().as_str(), e.description.as_ref().unwrap().as_str()), ("b", "c"));
        assert_eq!(e.due.clone().flatten().unwrap().date.as_str(), "2026-12-01");
        assert_eq!((e.after.as_slice(), e.reason.as_str()), ([tid(3)].as_slice(), "一；二"));
        // What changes nothing goes.
        let same = r#"{"suggestions": [{"number": 1, "title": "官网", "reason": ""}, {"number": 5, "done": true, "reason": ""}, {"order": [1, 2], "reason": ""}]}"#;
        assert!(parse(same, &shown).unwrap().is_empty());
    }

    #[test]
    fn refuses_what_could_not_be_applied() {
        let (_, shown) = prompt(&job(), snap());
        let err = |s: &str| parse(&format!(r#"{{"suggestions": [{s}]}}"#), &shown).err().unwrap();
        assert_eq!(err(r#"{"number": 6, "title": "x", "reason": ""}"#), "结果引用了不在这个文件夹里的事项 #6");
        assert_eq!(err(r#"{"number": 1, "requires": [2], "reason": ""}"#), "建议把 #2 设为 #1 的前置，但这样依赖会成环");
        // Turned round, there is no cycle.
        assert_eq!(
            parse(
                r#"{"suggestions": [{"number": 1, "requires": [2], "reason": ""}, {"number": 2, "drop_requires": [1], "reason": ""}]}"#,
                &shown
            )
            .unwrap()
            .len(),
            2
        );
        assert_eq!(err(r#"{"number": 1, "folder": "/别处", "reason": ""}"#), "建议把 #1 移到 /别处，但它不是这个文件夹下的子文件夹");
        assert_eq!(err(r#"{"number": 5, "due": "2026-11-20", "reason": ""}"#), "事项 #5 已完成，要先重新打开才能修改");
        // A task deleted takes its dependencies with it: none are suggested for it.
        let deleting = parse(
            r#"{"suggestions": [{"number": 2, "delete": true, "reason": ""}, {"number": 3, "requires": [2], "reason": ""}]}"#,
            &shown,
        );
        assert!(matches!(deleting.unwrap().as_slice(), [Suggestion::Delete(d)] if d.delete == tid(2)));
        assert_eq!(err(r#"{"number": 3, "remind": 10, "reason": ""}"#), "事项 #3：该事项没有具体的截止时间，无法设置提醒（remind）");
        assert_eq!(err(r#"{"title": "新的", "requires": ["b"], "reason": ""}"#), "结果里没有 ref 为“b”的新事项");
        assert_eq!(
            err(r#"{"number": 1, "required_by": [2], "reason": ""}"#),
            "修改事项 #1 的建议不能写 required_by，后续要写在后续事项自己的 requires 里"
        );
        assert_eq!(err(r#"{"order": [1, 5], "reason": ""}"#), "调整顺序只能排未完成的事项，#5 已完成");
        assert!(err(r#"{"number": 1, "title": "a", "note": "x"}"#).starts_with("模型返回的结果不符合格式"));
    }

    #[test]
    fn suggests_no_more_than_one_change_takes() {
        let tasks: Vec<Task> = (1..=50).map(|n| task(n, &format!("t{n}"), json!({}))).collect();
        let (_, shown) = prompt(&job(), Snapshot { tasks, folders: snap().folders, ..Default::default() });
        // Each task after every one before it: 1225 links, a DAG still.
        let all: Vec<Value> = (2..=50).map(|n| json!({ "number": n, "requires": (1..n).collect::<Vec<_>>(), "reason": "" })).collect();
        assert_eq!(
            parse(&json!({ "suggestions": all }).to_string(), &shown).err().unwrap(),
            "建议的修改太多，一次应用不了，请缩小范围后重试"
        );
    }
}
