//! Tidying a folder: what a job asks the model, and reading back what it suggests doing there, which
//! the user then takes or leaves part by part. See docs/ai.md#整理文件夹.

use std::collections::{HashMap, HashSet};

use nexttodo_core::board::{Board, Now, Snapshot};
use nexttodo_core::dag::find_path;
use nexttodo_core::model::{
    AiJob, Date, DefaultReminders, Description, Folder, FolderColor, FolderIcon, FolderId, FolderLook, NewTask, Period, Reorder, Repeat,
    RepeatFrom, Restyle, Suggestion, Task, TaskDeletion, TaskEdit, TaskId, Title, Was,
};
use nexttodo_core::ops::MAX_OPS;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::llm::Prompt;
use super::organize::{chat, due_text, unwrap};
use crate::error::from_value;
use crate::tools::args::{EditArgs, PlanRef, RemindText, RepeatText, TaskRef, WhenText, due_after_text, remind_text, task_ref};
use crate::tools::{Edit, edit_fields, new_due};

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
- looks：能改颜色和图标的文件夹，即这个文件夹和子文件夹，根目录和别人共享给我的文件夹本身除外；color、icon 是现在的，没写的是默认的；
- open_tasks：文件夹里未完成的事项，按现在的顺序。number 是编号；requires 是它已有的前置事项的编号，那些事项要先做完才能做它；due 是截止时间，due_after 是前置都完成后几天内截止（如 "30d"、"3d 18:00"，从前置都完成那天算），前置都完成以后才有 due，remind 是提醒（数字是提前多少分钟，"09:00" 是截止那天的几点，"1d 20:00" 是提前 1 天的 20:00，几个时是列表），repeat 是重复，写成间隔数加单位 d（天）、w（周）、m（月）或 y（年，就是 12 个月），如 1d 每天、2w 每两周、1m 每月、3m 每三个月、1y 每年；按周重复在某几天的，冒号后写周几（1 是周一，7 是周日），如 1w:1,2,3,4,5 每个工作日、1w:1,3,5 每周一三五、2w:2,4 每两周的周二和周四；按月重复在某几号的，冒号后写几号（1–31，没有这天的月份取最后一天），如 1m:1,15 每月 1 号和 15 号、1m:31 每月最后一天；repeat_from 为 done 表示下一次从完成那天算；repeat_until 是重复到哪天（含这天），做完这天以前的最后一次，事项就完成了；description_omitted 为 true 的事项描述太长，没有发来；
- done_tasks：最近完成的事项，只有编号和标题。

只输出一个 json 对象，不要有任何别的文字，格式如下：
{"suggestions": [
  {"number": 12, "title": "…", "description": "…", "due": "2026-11-20 18:00", "requires": [3], "reason": "…"},
  {"number": 15, "delete": true, "reason": "…"},
  {"ref": "a", "title": "…", "required_by": [12], "reason": "…"},
  {"order": [14, 12, 13], "reason": "…"},
  {"folder": "/学习/英语", "color": "blue", "icon": "book", "reason": "…"}
]}

每条建议是下面五种之一，都要写 reason，用一句话说明做什么、为什么。requires、required_by、drop_requires、clear、order 都是列表，只有一项也写成列表，如 [3]：
1. 修改已有的事项：写 number，再只写要改的字段，不改的省略，不要写空字符串或 null。同一个事项的几处修改写在同一条里，每个事项最多出现一次：
   - title：新标题，一行，简短（最好不超过 30 个字），以动作开头；原标题没说是什么动作的，不要为了以动作开头而猜一个动作或删词。
   - description：新描述，是改好后的完整内容，不是修改说明；Markdown，步骤可以写成清单“- [ ] ”。
   - due：截止时间，"YYYY-MM-DD" 或 "YYYY-MM-DD HH:MM"。原来有几点几分、这次只改日期的，带上原来的几点几分；原来和 request 都没说几点的，不要加；原来的提醒和重复保留，新的截止时间不带具体时间的，提前多久的提醒去掉，说了几点的留着。有 due_after 的事项，截止时间和提醒不要改，request 明说要改成某个日期的除外，那时写 due。
   - remind：提醒，整个换掉原来的。提前多久写 "30m"、"2h"、"1d"、"1w"（提前 30 分钟、2 小时、1 天、1 周）或分钟数，0 表示到截止时间时提醒，要有带具体时间的截止时间；说了在几点提醒的写几点："09:00" 是截止那天的几点，"1d 20:00" 是提前 1 天的 20:00。几个提醒写成列表。“不用提醒了”是去掉提醒，写 clear: ["remind"]，不是写 0。
   - repeat、repeat_from、repeat_until：重复，只在 request 要求设或改重复时写，写法同输入；要有截止时间，事项原来没有截止时间的，同时写 due（第一次）。按天数说的照天数写（每 90 天是 90d，不是 3m）；在一周里某几天的写周几，“每个工作日”是 1w:1,2,3,4,5，不是 1d（1d 周末也会出现）。request 说了重复到哪天为止的写 repeat_until，截止时间不能晚于它。这三项也只写要改的，不写的照旧，如只改间隔时不写 repeat_from 和 repeat_until；新设的重复不写 repeat_from 就是从截止日期算，不写 repeat_until 就是一直重复。
   - clear：要清除的设置，是一个列表，可以包含 "due"、"remind"、"repeat"、"repeat_until"（不再重复到哪天，一直重复下去），如 "clear": ["due"]；清除截止时间时，提醒和重复一起清除。去掉前置用 drop_requires，不用 clear。
   - done：true 标为完成（重复的事项会顺延到下一次，做的是 repeat_until 以前的最后一次的直接完成；request 说这件事整个做完了、不再重复的，如书读完了，同时在 clear 里写 "repeat"），false 重新打开 done_tasks 里的事项。done_tasks 里的事项可以删除；要改别的，先重新打开。
   - folder：移进 subfolders 里的哪个文件夹，写它的完整路径。subfolders 里没有的文件夹移不进去，这样的移动不要提。
   - requires：这一项要新加的前置，也就是要先做完、才能做这一项的事项：事项的编号，或新建事项的 ref；已有的不用写。
   - drop_requires：要去掉的这一项已有的前置，写编号。
2. 删除已有的事项：写 number 和 "delete": true。
3. 新建事项，放进这个文件夹：写 ref，依次是 "a"、"b"、"c"……，和 title；选填 description、due、remind、repeat、repeat_from、repeat_until，以及 requires（要先做完、才能做它的事项）和 required_by（要等它做完才能做的事项），写事项的编号或新建事项的 ref：新建的事项还没有编号，只能用 ref 指它。request 明确说新建的事项不用提醒的，写 "remind": []；没说提醒的不写 remind，会按用户自己设的默认提醒加上。
4. 调整顺序：order 是 open_tasks 里事项的编号，按新的顺序排；只写要调整的那些，它们在原来占着的位置之间重新排列。order 只改列表里的位置，前置总排在后续前面。request 要的顺序和已有的先后冲突时（如要把“上线”排到最前，而它要等“测试”），保留先后，只排没有冲突的那些；除非 request 明说要改先后，不要为了排序去掉或反转前置。
5. 改文件夹的颜色和图标：写 folder，照抄 looks 里的一个路径，不在 looks 里的文件夹不能改；再只写要改的 color、icon，每个文件夹最多出现一次。color 是 red、orange、amber、yellow、lime、green、teal、cyan、blue、indigo、purple、pink、brown、gray 之一，或 # 加六位十六进制数，如 #3478f6；icon 是 briefcase、graduation-cap、book、pen、file、laptop、code、flask、calculator、lightbulb、target、house、cart、utensils、coffee、shirt、baby、paw、heart、gift、dumbbell、bike、heart-pulse、pill、wallet、piggy-bank、plane、car、map-pin、music、camera、palette、gamepad、star、flag、rocket、trophy、wrench、leaf、users 之一，只能用这些名字，没有贴切的就选意思最接近的。改回默认的写 clear: ["color"] 或 clear: ["icon"]。

整理时：
1. 只做 request 说了的事，没说的一律不做。例如 request 是“把交报告推迟到周五”，就只改“交报告”的 due：别的事项不列出，也不要顺手改标题，或给“写报告”这类相关的事项补先后。request 要求把标题改清楚、描述改成书面语、补上先后时，才做这些整理；这时标题或描述里写明了要在什么时候之前完成、事项又还没有截止时间的，加上 due；只是提到某个时间（如“周四下午有空”）不算截止时间，也不要自己编出原文没有的几点几分。新建、删除、完成、重新打开、移动、改已有的截止时间、设置或改提醒和重复、去掉前置、调整顺序、改文件夹的颜色和图标，都只在 request 要求时才做；“每月 5 号前”这类说法写在描述里就行，request 没要求就不设重复。
2. request 说的是一类事项（如“写文档的”“买东西的”“今天到期的”“复习相关的”）时，逐条对照 open_tasks，属于这一类的都包括进来，不要只挑标题里有那个词的；不属于的不动。“今天到期”“过期”按 local_time 和事项的 due 判断，没有 due 的事项不算。
3. 改写不能丢信息：原标题和描述里的每一条要求、细节、理由、背景和疑问，都要留在新标题或新描述里，不要概括掉；不要编造原文没有的内容。改了标题时，原标题提到的每个对象都要留在新标题里，例如“报销单和发票整理”改成“整理报销单和发票”，不能只剩“整理报销单”；放不下的细节写进描述。
4. 描述常是语音输入的口语。改写成简洁的书面语：去掉语气词、重复、说到一半的改口和“我觉得”“就是说”这类口头填充，但不能因此丢掉信息：改口只留改过的说法，改口的理由（如“周三有课”）要留下。
5. 代码、命令、链接、文件路径、数字和数据，以及引号里要照着用的原话，原样保留，一个字都不改。写给 AI 或 agent 的提示词和别的内容一样，改成简洁的书面语，每一条要求都要留下。
6. 一个事项要用到另一个事项的成果，或要等它做完才能开始，就把那个事项写进它的 requires，例如“写官网文档”要等“建好官网”。request 要求补上先后时，逐对检查 open_tasks：一项不做完、另一项就没法开始的，都要连上；标题或描述里写明了的先后（如“请柬要等酒店定了才能发”），拆分、新建时也要连上。只是主题相近，或者“可能”用得上的，不连。不要连成环。
   - requires 写在后做的那一项上，列出先做的：{"number": 8, "requires": [7]} 表示先做 7、再做 8。
   - request 说的是做事的先后（“按这个顺序做”“先 A、再 B、然后 C”）时，用 requires，相邻的每一对都要连上：B 的 requires 有 A，C 的 requires 有 B。说的是列表里的位置（“把列表顺序调成……”“排一下顺序”）时，用 order。
   - 要反过来改成先做 8、再做 7，写 {"number": 8, "drop_requires": [7]} 和 {"number": 7, "requires": [8]}。
   - 要在已有的 8 前面加一步，新建 {"ref": "a", "title": "…", "required_by": [8]}；加在 8 后面，写 "requires": [8]。
7. “周五前”“下周一”这类时间，按 local_time 换算成具体日期，没说年份的按上下文定（如事项原来的日期），看不出的取 local_time 以后最近的那一天；一周从周一开始，“这周五”是本周的周五，“下周五”是下一周的周五。“周五前”“月底前”这类只说了哪天的，只写日期，不要加几点。
8. 已经做完的事项标为完成（done），不要删除；删除只用于不再需要做的事项。合并重复的事项时，留下一项，把另一项的信息并进它的标题或描述，再删除另一项；删掉的那项有前置或后续的，转到留下的那项上（后续写在后续自己的 requires 里），先后不能因合并而丢。
9. description_omitted 为 true 的事项和 done_tasks 里的事项，不要写 title 和 description：没看到描述，改标题可能丢掉信息。
10. 已经清楚的事项不要列出，不要为改而改：标题已经以动作开头、意思清楚的（如“交周报”“买牛奶”），不要换个同义的说法，如“交”改“提交”、“查”改“查阅”、“写”改“撰写”；没有要做的修改时输出 {"suggestions": []}。
11. 用原文的语言写。
12. 输出前逐条检查：
   - 每条建议都是 request 要求的；不是的删掉。
   - 把建议应用到事项上（没写的字段沿用原值），原标题和原描述的每一处都要能在改好后的标题或描述里找到，找不到的补上。
   - requires 写在后做的那一项上，里面是先做的事项；写反的改过来。"#;

/// Added to the instructions when a description shown refers to attachments.
pub const REFERENCES: &str = "有的描述里引用了附件，写成 [名称](attachment:…)：改写这些描述时，把引用原样留在新描述里，一个字都不改。";

/// What the model was shown: the data as it was then, as the job's user sees it, the folder, and
/// which of its tasks.
pub struct Shown {
    snap: Snapshot,
    user: String,
    /// When the board is as of: the job's in UTC, since tidying reads no task's status.
    now: Now,
    folder: Option<FolderId>,
    tasks: HashMap<i64, Seen>,
    /// The job's, for due dates the model gives with no word on reminders.
    reminders: Option<DefaultReminders>,
}

impl Shown {
    /// The reminders the job's device gives a task due `due`, if any.
    fn default_reminders(&self, due: Option<&WhenText>) -> Option<RemindText> {
        Some(RemindText(self.reminders.as_ref()?.for_due(due?.time.is_some())))
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Seen {
    /// Open, and whether with its description: only then may the model retitle it or rewrite that.
    Open {
        described: bool,
    },
    Done,
}

/// The chat for `job`, a tidying of its folder in `snap` with the user's `notes` along the way to it,
/// and what was shown in it.
pub fn prompt(job: &AiJob, snap: Snapshot, user: &str, notes: Vec<Value>) -> (Prompt, Shown) {
    let now = Now::utc(job.created_at);
    let board = Board::new(&snap, now.clone(), user);
    let all = board.tasks_in(job.folder_id);
    let open: Vec<&Task> = all.iter().copied().filter(|t| t.done_at.is_none()).take(SHOWN).collect();
    // Listed most recently done first.
    let done: Vec<&Task> = all.iter().copied().filter(|t| t.done_at.is_some()).take(DONE_SHOWN).collect();
    let numbers: HashSet<i64> = open.iter().map(|t| t.number).collect();
    let mut budget = DESCRIBED;
    let mut referring = false;
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
                referring |= references(text).next().is_some();
                v["description"] = text.into();
            }
            if let Some(d) = &t.due {
                v["due"] = due_text(d).into();
                if let Some(r) = &d.repeat {
                    v["repeat"] = repeat_text(r).into();
                    if r.from == RepeatFrom::Done {
                        v["repeat_from"] = "done".into();
                    }
                    if let Some(until) = &r.until {
                        v["repeat_until"] = until.as_str().into();
                    }
                }
            }
            if let Some(a) = &t.due_after {
                v["due_after"] = due_after_text(a).into();
            }
            match t.reminders().as_slice() {
                [] => {}
                [one] => v["remind"] = remind_text(one),
                all => v["remind"] = all.iter().map(remind_text).collect(),
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
    let inside: Vec<Option<FolderId>> =
        board.folder_tree().into_iter().filter(|&f| f != job.folder_id && board.in_subtree(f, job.folder_id)).take(FOLDERS_SHOWN).collect();
    let subfolders: Vec<String> = inside.iter().map(|&f| board.path(f)).collect();
    // Those it may restyle, by the paths it is to write. Told in words which it may not, models
    // restyled those anyway, and made up paths.
    let looks: Vec<Value> = std::iter::once(job.folder_id)
        .chain(inside)
        .filter_map(|f| {
            let folder = board.folder(f?)?;
            if !stylable(&board, job.folder_id, folder) {
                return None;
            }
            let mut v = json!({ "folder": board.path(f) });
            if let Some(c) = folder.color {
                v["color"] = String::from(c).into();
            }
            if let Some(i) = folder.icon {
                v["icon"] = i.as_str().into();
            }
            Some(v)
        })
        .collect();
    let mut input = json!({
        "local_time": job.local_time.as_str(),
        "folder": board.path(job.folder_id),
        "request": job.text.as_str(),
        "subfolders": subfolders,
        "open_tasks": open_tasks,
        "done_tasks": done_tasks,
    });
    if !looks.is_empty() {
        input["looks"] = looks.into();
    }
    if !notes.is_empty() {
        input["notes"] = notes.into();
    }
    let mut prompt = chat(SYSTEM, &input);
    if referring {
        prompt.system = format!("{}\n\n{REFERENCES}", prompt.system);
    }
    (prompt, Shown { snap, user: user.to_owned(), now, folder: job.folder_id, tasks, reminders: job.reminders.clone() })
}

/// The references to attachments in a description, `[名称](attachment:<ID>)` with `[`, `]` and `\`
/// escaped in the name (docs/design.md#附件): each whole, with its id.
fn references(text: &str) -> impl Iterator<Item = (&str, &str)> {
    const MARK: &str = "](attachment:";
    text.match_indices(MARK).filter_map(move |(at, _)| {
        let rest = &text[at + MARK.len()..];
        let id = &rest[..rest.find(')')?];
        Uuid::parse_str(id).ok()?;
        // The name starts after the last `[` before that is not escaped.
        let start = text[..at]
            .rmatch_indices('[')
            .map(|(i, _)| i)
            .find(|&i| text[..i].chars().rev().take_while(|&c| c == '\\').count() % 2 == 0)?;
        Some((&text[start..at + MARK.len() + id.len() + 1], id))
    })
}

/// The description `new` in place of `was`, with the references to attachments `was` has and it lacks
/// put at the end: the model may drop one, and the attachment would be referred to no more.
fn with_references(was: &Description, new: Description) -> Result<Description, String> {
    let missing: Vec<&str> = {
        let mut have: HashSet<&str> = references(new.as_str()).map(|(_, id)| id).collect();
        references(was.as_str()).filter(|&(_, id)| have.insert(id)).map(|(r, _)| r).collect()
    };
    if missing.is_empty() {
        return Ok(new);
    }
    let text = String::from(new);
    let gap = if text.trim().is_empty() { "" } else { "\n\n" };
    Description::try_from(format!("{}{gap}{}", text.trim_end(), missing.join(" ")))
}

/// `1d`, `2w`, `3m`, `1w:1,3,5`, `1m:1,15`, `12lm:15`, as the tools take a repeat.
pub fn repeat_text(r: &Repeat) -> String {
    let (unit, days) = match r.period {
        Period::Day => ("d", None),
        Period::Week(days) => ("w", days.map(Vec::from)),
        Period::Month(days) => ("m", days.map(Vec::from)),
        Period::LunarMonth(days) => ("lm", Some(Vec::from(days))),
    };
    let days = days.map(|d| format!(":{}", d.iter().map(u8::to_string).collect::<Vec<_>>().join(",")));
    format!("{}{unit}{}", r.every.get(), days.unwrap_or_default())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    suggestions: Vec<Suggested>,
}

/// A suggestion as the model writes it: a change to the task `number`, its deletion, a new task, an
/// `order`, or a `folder`'s colour and icon.
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
    due: Option<WhenText>,
    remind: Option<RemindText>,
    repeat: Option<RepeatText>,
    repeat_from: Option<RepeatFrom>,
    repeat_until: Option<Date>,
    #[serde(default)]
    clear: Vec<Setting>,
    done: Option<bool>,
    folder: Option<String>,
    color: Option<FolderColor>,
    icon: Option<FolderIcon>,
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
        self.of_task() || self.folder.is_some() || self.styles()
    }

    /// Whether it changes anything of a task's own, the folder it is in aside.
    fn of_task(&self) -> bool {
        self.delete
            || self.title.is_some()
            || self.description.is_some()
            || self.due.is_some()
            || self.remind.is_some()
            || self.repeat.is_some()
            || self.repeat_from.is_some()
            || self.repeat_until.is_some()
            || self.clear.iter().any(|c| !matches!(c, Setting::Color | Setting::Icon))
            || self.done.is_some()
            || !self.after.is_empty()
            || !self.before.is_empty()
            || !self.unlink.is_empty()
    }

    /// Whether it gives a folder a colour or an icon.
    fn styles(&self) -> bool {
        self.color.is_some() || self.icon.is_some() || self.clear.iter().any(|c| matches!(c, Setting::Color | Setting::Icon))
    }

    /// The one suggestion `self` and `later`, for the same task or folder, make: what both change, the later
    /// one's way where they differ.
    fn merge(&mut self, later: Suggested) {
        if !later.says() {
            return;
        }
        self.reason = match (self.says(), self.reason.is_empty(), later.reason.is_empty()) {
            (true, false, false) => format!("{}；{}", self.reason.trim_end_matches('。'), later.reason),
            (_, _, false) => later.reason,
            _ => std::mem::take(&mut self.reason),
        };
        self.delete |= later.delete;
        // A setting one clears and the other sets is as the later one says.
        self.clear.retain(|c| match c {
            Setting::Due => later.due.is_none(),
            Setting::Remind => later.remind.is_none(),
            Setting::Repeat => later.repeat.is_none(),
            Setting::RepeatUntil => later.repeat_until.is_none(),
            Setting::Color => later.color.is_none(),
            Setting::Icon => later.icon.is_none(),
        });
        self.unset(&later.clear);
        self.title = later.title.or(self.title.take());
        self.description = later.description.or(self.description.take());
        self.due = later.due.or(self.due.take());
        self.remind = later.remind.or(self.remind.take());
        self.repeat = later.repeat.or(self.repeat);
        self.repeat_from = later.repeat_from.or(self.repeat_from);
        self.repeat_until = later.repeat_until.or(self.repeat_until.take());
        self.clear.extend(later.clear);
        self.done = later.done.or(self.done);
        self.folder = later.folder.or(self.folder.take());
        self.color = later.color.or(self.color);
        self.icon = later.icon.or(self.icon);
        self.after.extend(later.after);
        self.before.extend(later.before);
        self.unlink.extend(later.unlink);
    }

    /// Drops what `clear` clears, as `edit` would: the due date's reminder and repeat with it.
    fn unset(&mut self, clear: &[Setting]) {
        for c in clear {
            match c {
                Setting::Due => (self.due, self.remind, self.repeat, self.repeat_from, self.repeat_until) = (None, None, None, None, None),
                Setting::Remind => self.remind = None,
                Setting::Repeat => (self.repeat, self.repeat_from, self.repeat_until) = (None, None, None),
                Setting::RepeatUntil => self.repeat_until = None,
                Setting::Color => self.color = None,
                Setting::Icon => self.icon = None,
            }
        }
    }
}

/// What `clear` clears: as `null` does for `edit`, which models write for what they leave alone.
#[derive(Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Setting {
    Due,
    Remind,
    Repeat,
    RepeatUntil,
    Color,
    Icon,
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
    let board = Board::new(&shown.snap, shown.now.clone(), &shown.user);
    // Models split what they suggest for a task, or a folder, at times, however they are told.
    let mut suggestions: Vec<Suggested> = Vec::new();
    for mut s in all {
        // Before it is merged into another, which would hide what does not go with a folder's look.
        if s.styles() && (s.order.is_some() || s.number.is_some() || s.name.is_some() || s.of_task()) {
            return Err("修改文件夹颜色和图标的建议只能写 folder、color、icon、clear 和 reason".into());
        }
        // A setting one suggestion both sets and clears is cleared.
        s.unset(&s.clear.clone());
        let folder = |x: &Suggested| x.folder.as_deref().and_then(|p| board.find_folder(p));
        let same = |x: &Suggested| {
            x.order.is_none()
                && s.order.is_none()
                && match x.number {
                    Some(_) => x.number == s.number,
                    None => x.styles() && s.styles() && folder(x).is_some() && folder(x) == folder(&s),
                }
        };
        match suggestions.iter_mut().find(|x| same(x)) {
            Some(first) => first.merge(s),
            None => suggestions.push(s),
        }
    }
    // The folder's tasks all have its owner's numbers.
    let owner = board.owner_of(shown.folder);
    let task = |n: i64| -> Result<(&Task, Seen), String> {
        let seen = *shown.tasks.get(&n).ok_or_else(|| format!("结果引用了不在这个文件夹里的事项 #{n}"))?;
        Ok((board.task_by_number(owner, n).expect("shown tasks are in the snapshot"), seen))
    };
    // New tasks get their ids first: suggestions before them may refer to them.
    let mut refs = HashMap::new();
    let mut new = HashMap::new();
    let ids: Vec<Option<TaskId>> =
        suggestions.iter().map(|s| (s.number.is_none() && s.order.is_none() && !s.styles()).then(|| TaskId(Uuid::now_v7()))).collect();
    for (s, id) in suggestions.iter().zip(&ids) {
        let (Some(id), Some(name)) = (id, &s.name) else { continue };
        if task_ref(name).is_some() {
            return Err(format!("新事项的 ref“{name}”会被当成事项编号"));
        }
        if refs.insert(name.clone(), *id).is_some() {
            return Err(format!("结果里有多个新事项的 ref 都是“{name}”"));
        }
    }
    let resolve = |r: &PlanRef| -> Result<TaskId, String> {
        match r {
            PlanRef::Number(n) => Ok(task(*n)?.0.id),
            PlanRef::Name(s) => match task_ref(s) {
                Some((None, n)) => Ok(task(n)?.0.id),
                Some((Some(_), _)) => Err(format!("“{s}”不是这个文件夹里的事项")),
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
        if s.styles() {
            out.extend(restyle(&board, shown, s)?.map(Suggestion::Style));
            continue;
        }
        let Some(n) = s.number else {
            let id = id.expect("new tasks have ids");
            let title = s.title.ok_or("新事项缺少标题")?;
            if s.delete || s.done.is_some() || s.folder.is_some() || !s.clear.is_empty() || !s.unlink.is_empty() {
                return Err(format!(
                    "新事项“{}”只能写 title、description、due、remind、repeat、repeat_from、repeat_until、requires 和 required_by",
                    title.as_str()
                ));
            }
            let remind = s.remind.or_else(|| shown.default_reminders(s.due.as_ref()));
            let (due, _) = new_due(s.due, None, remind, s.repeat, s.repeat_from, s.repeat_until)
                .map_err(|e| format!("新事项“{}”：{}", title.as_str(), e.describe()))?;
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
        let was = Was { title: t.title.clone(), description: t.description.clone(), due: t.due.clone(), due_after: t.due_after.clone() };
        if s.delete {
            deleted.insert(t.id);
            out.push(Suggestion::Delete(TaskDeletion { delete: t.id, reason: s.reason, was }));
            continue;
        }
        // Not having seen the description, it would lose what the title held that this lacks.
        let described = state == Seen::Open { described: true };
        let title = s.title.filter(|_| described);
        let description = s
            .description
            .filter(|_| described)
            .map(|d| with_references(&t.description, d))
            .transpose()
            .map_err(|e| format!("事项 #{n}：{e}"))?;
        let current = t.due.as_ref().and_then(|d| d.repeat.as_ref());
        // A last day cleared where there is none changes nothing.
        let clears = s.clear.iter().any(|&c| c != Setting::RepeatUntil || current.is_some_and(|r| r.until.is_some()));
        let fields = title.is_some()
            || description.is_some()
            || s.due.is_some()
            || s.remind.is_some()
            || s.repeat.is_some()
            || s.repeat_from.is_some()
            || s.repeat_until.is_some()
            || clears;
        let Edit { title, description, due, .. } = if fields {
            let clear = |x| s.clear.contains(&x);
            // The parts of a repeat given, the rest as they are, as models leave out what they keep;
            // edits take a repeat whole.
            let (repeat, repeat_from, repeat_until) = match current {
                Some(r)
                    if !clear(Setting::Due)
                        && !clear(Setting::Repeat)
                        && (s.repeat.is_some() || s.repeat_from.is_some() || s.repeat_until.is_some() || clear(Setting::RepeatUntil)) =>
                {
                    (
                        Some(s.repeat.unwrap_or(RepeatText { every: r.every, period: r.period })),
                        s.repeat_from.or(Some(r.from)),
                        if clear(Setting::RepeatUntil) { None } else { s.repeat_until.or_else(|| r.until.clone()) },
                    )
                }
                _ => (s.repeat, s.repeat_from, s.repeat_until),
            };
            // As a new task's when it first gets a due date.
            let remind = match s.remind {
                _ if clear(Setting::Remind) => Some(None),
                Some(r) => Some(Some(r)),
                None => (t.due.is_none() && t.due_after.is_none()).then(|| shown.default_reminders(s.due.as_ref())).flatten().map(Some),
            };
            let args = EditArgs {
                task: TaskRef { user: None, number: n },
                title,
                description,
                replace: Vec::new(),
                append: None,
                start: None,
                due: if clear(Setting::Due) { Some(None) } else { s.due.map(Some) },
                due_after: None,
                remind,
                repeat: if clear(Setting::Repeat) { Some(None) } else { repeat.map(Some) },
                repeat_from,
                repeat_until,
                assign: Vec::new(),
                unassign: Vec::new(),
            };
            edit_fields(&board, t, args).map_err(|e| format!("事项 #{n}：{}", e.describe()))?
        } else {
            Edit {
                id: t.id,
                title: None,
                description: None,
                start: None,
                due: None,
                due_after: None,
                assign: Vec::new(),
                unassign: Vec::new(),
            }
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
            // One due some days after its prerequisites takes a date of its own only (docs/ai.md#整理文件夹),
            // which is a change also when the same as the one they gave.
            due: due.filter(|d| if t.due_after.is_some() { d.is_some() } else { *d != t.due }),
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
        out.push(Suggestion::Edit(Box::new(edit)));
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
            Suggestion::Delete(_) | Suggestion::Order(_) | Suggestion::Style(_) => {}
        }
    }
    out.retain(|s| match s {
        Suggestion::Edit(e) => changes(e) || e.done.is_some(),
        Suggestion::Style(r) => r.color.is_some() || r.icon.is_some(),
        Suggestion::Delete(_) | Suggestion::Create(_) | Suggestion::Order(_) => true,
    });
    let mut links = Vec::new();
    let mut unlinks = HashSet::new();
    for s in &out {
        match s {
            Suggestion::Edit(e) => {
                links.extend(e.after.iter().map(|&b| (b, e.task)));
                unlinks.extend(e.unlink.iter().map(|&b| (b, e.task)));
            }
            Suggestion::Create(c) => links.extend(c.after.iter().map(|&b| (b, c.create)).chain(c.before.iter().map(|&a| (c.create, a)))),
            Suggestion::Delete(_) | Suggestion::Order(_) | Suggestion::Style(_) => {}
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
            Suggestion::Delete(_) | Suggestion::Style(_) => 1,
            Suggestion::Create(c) => 1 + c.after.len() + c.before.len(),
            Suggestion::Order(o) => o.order.len(),
        })
        .sum();
    if ops + 1 > MAX_OPS {
        return Err("建议的修改太多，一次应用不了，请缩小范围后重试".into());
    }
    Ok(out)
}

/// The colour and icon `s` gives a folder: the one tidied or one inside it, leaving out what it has
/// already. None for the root, which has none, and one not [`stylable`]: not in `looks`, but models
/// restyle them when asked to, and the rest still holds.
fn restyle(board: &Board, shown: &Shown, s: Suggested) -> Result<Option<Restyle>, String> {
    let path = s.folder.as_deref().ok_or("修改颜色和图标的建议缺少 folder")?;
    let f = match board.find_folder(path) {
        Some(None) => return Ok(None),
        Some(Some(id)) if board.in_subtree(Some(id), shown.folder) => board.folder(id).expect("found by path"),
        _ => return Err(format!("建议修改 {path} 的颜色和图标，但它不是这个文件夹或其中的子文件夹")),
    };
    if !stylable(board, shown.folder, f) {
        return Ok(None);
    }
    let clear = |x| s.clear.contains(&x);
    Ok(Some(Restyle {
        style: f.id,
        color: if clear(Setting::Color) { Some(None) } else { s.color.map(Some) }.filter(|&c| c != f.color),
        icon: if clear(Setting::Icon) { Some(None) } else { s.icon.map(Some) }.filter(|&i| i != f.icon),
        reason: s.reason,
        was: FolderLook { color: f.color, icon: f.icon },
    }))
}

/// Whether a tidying of `tidied` may restyle `f`, a folder inside it. Not a folder someone shares with
/// the user, which only they can restyle; what is inside it, only when tidying in there, not the root.
fn stylable(board: &Board, tidied: Option<FolderId>, f: &Folder) -> bool {
    board.owner_of(Some(f.id)) == board.owner_of(tidied) && !board.shared_with_me().iter().any(|x| x.id == f.id)
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
    use nexttodo_core::model::{ColorPreset, Dependency, Person, Share};

    fn id(n: i64) -> String {
        format!("00000000-0000-7000-8000-{n:012}")
    }

    fn task(n: i64, title: &str, extra: Value) -> Task {
        let mut v = json!({
            "id": id(n), "number": n, "folder_id": id(100), "title": title, "description": "", "done_at": null, "due": null,
            "sort_key": format!("a{n:02}1"), "assignees": [], "created_at": 0, "updated_at": 0, "created_by": "me", "updated_by": "me", "created_via": null, "updated_via": null,
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
            task(2, "写官网文档", json!({ "due": { "date": "2026-11-20", "time": "18:00", "reminders": [{ "before": 30 }] } })),
            task(3, "长的", json!({ "description": long })),
            task(
                4,
                "也长",
                json!({ "description": long, "due": { "date": "2026-11-09", "repeat": { "every": 1, "unit": "week", "from": "done", "until": "2026-12-31" } } }),
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
        let (prompt, shown) = prompt(&job(), snap(), "me", vec![]);
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
            json!({ "number": 4, "title": "也长", "description_omitted": true, "due": "2026-11-09", "repeat": "1w", "repeat_from": "done", "repeat_until": "2026-12-31" })
        );
        assert_eq!(input["done_tasks"], json!([{ "number": 5, "title": "订机票" }]));
        assert_eq!(input["open_tasks"].as_array().unwrap().len(), 4);
        assert!(shown.tasks[&3] == Seen::Open { described: true } && shown.tasks[&4] == Seen::Open { described: false });
        assert!(input.get("notes").is_none() && prompt.system == SYSTEM, "no notes, no word of them");
    }

    #[test]
    fn gives_the_notes_with_what_to_make_of_them() {
        let notes = vec![json!({ "folder": "/", "text": "标题用英文" }), json!({ "folder": "/官网", "text": "按页面拆开" })];
        let (prompt, _) = prompt(&job(), snap(), "me", notes.clone());
        let input: Value = serde_json::from_str(&prompt.user).unwrap();
        assert_eq!(input["notes"], json!(notes));
        assert_eq!(prompt.system, format!("{SYSTEM}\n\n{}", super::super::organize::NOTES));
    }

    #[test]
    fn reads_each_kind() {
        let (_, shown) = prompt(&job(), snap(), "me", vec![]);
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
        assert_eq!((due.date.as_str(), due.at.unwrap().remind_before), ("2026-11-21", vec![30]));
        assert_eq!(b.folder, Some(FolderId(id(101).parse().unwrap())));
        assert!(b.after.is_empty() && b.unlink.is_empty());
        // Not having seen the description, no new title.
        assert!(c.title.is_none() && c.done == Some(true));
        assert_eq!(c.due.clone().flatten().unwrap().repeat, None);
        assert_eq!(d.delete, tid(3));
        assert_eq!(e.before, [tid(2)]);
        assert_eq!(e.due.as_ref().unwrap().at.as_ref().unwrap().remind_before, [60]);
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
        // The parts of a repeat given change, the rest stays, also given in two.
        let repeat = |answer: &str, shown: &Shown| {
            let s = parse(answer, shown).unwrap();
            let [Suggestion::Edit(e)] = s.as_slice() else { panic!("{s:?}") };
            let r = e.due.clone().flatten().unwrap().repeat.unwrap();
            (r.every.get(), r.from, r.until.map(String::from))
        };
        let twice =
            r#"{"suggestions": [{"number": 4, "repeat_until": "2026-12-19", "reason": ""}, {"number": 4, "repeat": "2w", "reason": ""}]}"#;
        assert_eq!(repeat(twice, &shown), (2, RepeatFrom::Done, Some("2026-12-19".into())));
        let cleared = r#"{"suggestions": [{"number": 4, "clear": ["repeat_until"], "reason": ""}]}"#;
        assert_eq!(repeat(cleared, &shown), (1, RepeatFrom::Done, None));
        // The due date cleared takes its repeat along, also what an earlier suggestion set of it.
        for answer in [
            r#"{"suggestions": [{"number": 4, "clear": ["due"], "reason": ""}]}"#,
            r#"{"suggestions": [{"number": 4, "clear": ["due", "repeat_until"], "reason": ""}]}"#,
            r#"{"suggestions": [{"number": 4, "repeat_until": "2026-12-19", "reason": ""}, {"number": 4, "clear": ["due"], "reason": ""}]}"#,
        ] {
            let s = parse(answer, &shown).unwrap();
            let [Suggestion::Edit(e)] = s.as_slice() else { panic!("{s:?}") };
            assert_eq!(e.due, Some(None), "{answer}");
        }
        // Set and cleared in one suggestion, it is cleared, whether the task repeated or not.
        let both = r#"{"suggestions": [{"number": NUMBER, "repeat": "2d", "repeat_until": "2026-12-19", "clear": ["repeat_until"], "reason": ""}]}"#;
        assert_eq!(repeat(&both.replace("NUMBER", "4"), &shown), (2, RepeatFrom::Done, None));
        let (_, nothing) =
            prompt(&job(), Snapshot { tasks: vec![task(1, "x", json!({ "due": { "date": "2026-11-09" } }))], ..snap() }, "me", vec![]);
        assert_eq!(repeat(&both.replace("NUMBER", "1"), &nothing), (2, RepeatFrom::Due, None));
        // A last day cleared where there is none changes nothing.
        assert!(parse(r#"{"suggestions": [{"number": 1, "clear": ["repeat_until"], "reason": ""}]}"#, &nothing).unwrap().is_empty());
        // What changes nothing goes.
        let same = r#"{"suggestions": [{"number": 1, "title": "官网", "reason": ""}, {"number": 5, "done": true, "reason": ""}, {"order": [1, 2], "reason": ""}]}"#;
        assert!(parse(same, &shown).unwrap().is_empty());
    }

    #[test]
    fn keeps_the_references_to_attachments_a_new_description_drops() {
        let mut s = snap();
        let (picture, sheet) = (
            "[图片 \\[1\\].png](attachment:00000000-0000-7000-8000-000000000201)",
            "[表格.csv](attachment:00000000-0000-7000-8000-000000000202)",
        );
        s.tasks[0].description = Description::try_from(format!("看 {picture}，还有 {sheet} 和 {picture}")).unwrap();
        let (prompt, shown) = prompt(&job(), s, "me", vec![]);
        assert_eq!(prompt.system, format!("{SYSTEM}\n\n{REFERENCES}"));
        // What is left of the picture's is no reference.
        let rewritten = format!("做官网，见 {sheet}，图见 (attachment:00000000-0000-7000-8000-000000000201)");
        let answer = format!(r#"{{"suggestions": [{{"number": 1, "description": "{rewritten}", "reason": ""}}]}}"#);
        let suggested = parse(&answer, &shown).unwrap();
        let [Suggestion::Edit(e)] = suggested.as_slice() else { panic!("{suggested:?}") };
        assert_eq!(e.description.as_ref().unwrap().as_str(), format!("{rewritten}\n\n{picture}"));
    }

    #[test]
    fn refuses_what_could_not_be_applied() {
        let (_, shown) = prompt(&job(), snap(), "me", vec![]);
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
        assert_eq!(err(r#"{"number": 3, "remind": 10, "reason": ""}"#), "事项 #3：该事项没有截止时间，无法设置提醒（remind）");
        // Due after the last day it would repeat on, a suggestion can't be taken.
        assert_eq!(
            err(r#"{"number": 4, "repeat": "1w", "repeat_until": "2026-11-01", "reason": ""}"#),
            "事项 #4：重复到不能早于截止日期（repeat_until）"
        );
        assert_eq!(
            err(r#"{"title": "新的", "due": "2026-12-20", "repeat": "1d", "repeat_until": "2026-12-19", "reason": ""}"#),
            "新事项“新的”：重复到不能早于截止日期（due）"
        );
        assert_eq!(err(r#"{"title": "新的", "requires": ["b"], "reason": ""}"#), "结果里没有 ref 为“b”的新事项");
        assert_eq!(
            err(r#"{"number": 1, "required_by": [2], "reason": ""}"#),
            "修改事项 #1 的建议不能写 required_by，后续要写在后续事项自己的 requires 里"
        );
        assert_eq!(err(r#"{"order": [1, 5], "reason": ""}"#), "调整顺序只能排未完成的事项，#5 已完成");
        assert!(err(r#"{"number": 1, "title": "a", "note": "x"}"#).starts_with("模型返回的结果不符合格式"));
    }

    #[test]
    fn restyles_the_folder_and_those_inside_it() {
        let blue = FolderColor::try_from("blue".to_owned()).unwrap();
        let mut s = snap();
        s.folders[1].color = Some(blue);
        let (chat, shown) = prompt(&job(), s.clone(), "me", vec![]);
        let input: Value = serde_json::from_str(&chat.user).unwrap();
        assert_eq!(input["looks"], json!([{ "folder": "/官网" }, { "folder": "/官网/文档", "color": "blue" }]));
        let styles = |s: &str| parse(&format!(r#"{{"suggestions": [{s}]}}"#), &shown);
        // Split, and the path written another way, it is one; what it has already is left out.
        let got = styles(
            r#"{"folder": "/官网/文档", "icon": "book", "color": "blue", "reason": "配上图标。"}, {"folder": "官网/文档/", "color": "green", "reason": "改成绿色。"}"#,
        )
        .unwrap();
        let [Suggestion::Style(r)] = got.as_slice() else { panic!("{got:?}") };
        assert_eq!(r.style, s.folders[1].id);
        assert_eq!((r.color.map(|c| c.map(String::from)), r.icon), (Some(Some("green".into())), Some(Some(FolderIcon::Book))));
        assert_eq!((r.was.color, r.was.icon, r.reason.as_str()), (Some(blue), None, "配上图标；改成绿色。"));
        let got = styles(r#"{"folder": "/官网", "clear": ["color"], "icon": "code", "reason": ""}"#).unwrap();
        assert!(matches!(got.as_slice(), [Suggestion::Style(r)] if r.color.is_none() && r.icon == Some(Some(FolderIcon::Code))));
        let got = styles(r#"{"folder": "/官网/文档", "clear": ["color"], "reason": ""}"#).unwrap();
        assert!(matches!(got.as_slice(), [Suggestion::Style(r)] if r.color == Some(None)));
        // Kept with the job, back to the default stays that, apart from leaving it as it is.
        let kept: Vec<Suggestion> = serde_json::from_value(serde_json::to_value(&got).unwrap()).unwrap();
        assert_eq!(kept, got);
        assert!(styles(r#"{"folder": "/官网/文档", "color": "blue", "reason": ""}"#).unwrap().is_empty(), "as it is");
        let err = |s: &str| styles(s).err().unwrap();
        assert_eq!(
            err(r#"{"folder": "/别处", "color": "red", "reason": ""}"#),
            "建议修改 /别处 的颜色和图标，但它不是这个文件夹或其中的子文件夹"
        );
        let mixed = "修改文件夹颜色和图标的建议只能写 folder、color、icon、clear 和 reason";
        assert_eq!(err(r#"{"number": 1, "folder": "/官网", "color": "red", "reason": ""}"#), mixed);
        assert_eq!(err(r#"{"order": [2, 1], "color": "red", "reason": ""}"#), mixed);
        // Not hidden by merging into a suggestion for the same folder.
        assert_eq!(
            err(r#"{"folder": "/官网", "color": "red", "reason": ""}, {"folder": "/官网", "ref": "a", "icon": "book", "reason": ""}"#),
            mixed
        );
        assert_eq!(err(r#"{"icon": "book", "reason": ""}"#), "修改颜色和图标的建议缺少 folder");

        // Shared with the user, the folder itself is its owner's to restyle, and not shown as one to; what
        // is inside it is the user's too. Suggested all the same, it is left out, and the rest kept.
        s.shares = vec![Share { folder_id: s.folders[0].id, owner: "alice".into(), members: vec!["me".into()] }];
        s.people = vec![Person { id: "alice".into(), username: "alice".into(), display_name: "Alice".into() }];
        let (chat, shown) = prompt(&job(), s.clone(), "me", vec![]);
        let input: Value = serde_json::from_str(&chat.user).unwrap();
        assert_eq!(input["looks"], json!([{ "folder": "@alice/官网/文档", "color": "blue" }]));
        let styles = |s: &str| parse(&format!(r#"{{"suggestions": [{s}]}}"#), &shown);
        let got = styles(
            r#"{"folder": "@alice/官网", "icon": "book", "reason": ""}, {"folder": "@alice/官网/文档", "icon": "book", "reason": ""}"#,
        );
        assert!(matches!(got.unwrap().as_slice(), [Suggestion::Style(r)] if r.style == s.folders[1].id));
        assert!(styles(r#"{"folder": "/", "icon": "book", "reason": ""}"#).unwrap().is_empty(), "the root has none");
        // Tidying the root, not what is inside it either: only the user's own.
        let (chat, shown) = prompt(&AiJob { folder_id: None, ..job() }, s.clone(), "me", vec![]);
        let input: Value = serde_json::from_str(&chat.user).unwrap();
        assert_eq!(input["looks"], json!([{ "folder": "/别处" }]));
        let got = parse(
            r#"{"suggestions": [{"folder": "@alice/官网/文档", "icon": "book", "reason": ""}, {"folder": "/别处", "icon": "book", "reason": ""}]}"#,
            &shown,
        );
        assert!(matches!(got.unwrap().as_slice(), [Suggestion::Style(r)] if r.style == s.folders[2].id));
        // The user's own, shared with others, is theirs.
        s.shares[0] = Share { folder_id: s.folders[0].id, owner: "me".into(), members: vec!["alice".into()] };
        let (_, shown) = prompt(&job(), s, "me", vec![]);
        assert_eq!(parse(r#"{"suggestions": [{"folder": "/官网", "icon": "book", "reason": ""}]}"#, &shown).unwrap().len(), 1);
    }

    #[test]
    fn lists_the_colours_and_icons_there_are() {
        let listed = |after: &str| {
            let rest = &SYSTEM[SYSTEM.find(after).unwrap() + after.len()..];
            rest[..rest.find(" 之一").unwrap()].split('、').map(str::to_owned).collect::<Vec<_>>()
        };
        assert_eq!(listed("color 是 "), ColorPreset::ALL.iter().map(|c| c.as_str()).collect::<Vec<_>>());
        assert_eq!(listed("icon 是 "), FolderIcon::ALL.iter().map(|i| i.as_str()).collect::<Vec<_>>());
    }

    #[test]
    fn takes_only_a_date_of_its_own_for_a_task_due_some_days_after_its_prerequisites() {
        let tasks = vec![
            task(1, "拿到发票", json!({})),
            task(2, "报销", json!({ "due_after": { "days": 7 } })),
            task(3, "复诊", json!({ "due_after": { "days": 7 }, "unlocked_on": "2026-11-25", "due": { "date": "2026-12-02" } })),
        ];
        let dependencies = vec![Dependency { before: tasks[0].id, after: tasks[1].id, created_at: 0 }];
        let (_, shown) = prompt(&job(), Snapshot { tasks, dependencies, folders: snap().folders, ..Default::default() }, "me", vec![]);
        let answer = json!({ "suggestions": [
            { "number": 2, "clear": ["due"], "reason": "" },
            { "number": 3, "due": "2026-12-02", "reason": "" },
        ] });
        let dues: Vec<_> = parse(&answer.to_string(), &shown)
            .unwrap()
            .into_iter()
            .map(|s| match s {
                Suggestion::Edit(e) => (e.task, json!(e.due)),
                _ => panic!("edits only"),
            })
            .collect();
        assert_eq!(dues, [(tid(3), json!({ "date": "2026-12-02" }))]);
    }

    #[test]
    fn suggests_no_more_than_one_change_takes() {
        let tasks: Vec<Task> = (1..=50).map(|n| task(n, &format!("t{n}"), json!({}))).collect();
        let (_, shown) = prompt(&job(), Snapshot { tasks, folders: snap().folders, ..Default::default() }, "me", vec![]);
        // Each task after every one before it: 1225 links, a DAG still.
        let all: Vec<Value> = (2..=50).map(|n| json!({ "number": n, "requires": (1..n).collect::<Vec<_>>(), "reason": "" })).collect();
        assert_eq!(
            parse(&json!({ "suggestions": all }).to_string(), &shown).err().unwrap(),
            "建议的修改太多，一次应用不了，请缩小范围后重试"
        );
    }
}
