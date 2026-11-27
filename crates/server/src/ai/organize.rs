//! What a job asks the model, and reading back the tasks it answers with.

use std::collections::HashMap;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use nexttodo_core::board::Board;
use nexttodo_core::model::{
    AiJob, Attachment, AttachmentId, CompletionMark, Date, DefaultReminders, Due, FileSize, FolderId, Task, TaskId,
};
use nexttodo_core::ops::Op;
use serde::Deserialize;
use serde_json::{Value, json};

use super::llm::{Picture, Prompt};
use crate::attachments::showable_image;
use crate::error::{ApiError, from_value};
use crate::files::Files;
use crate::tools::args::{PlanRef, PlanTask, RemindText, due_after_text, task_ref};

/// How many of the folder's open tasks the model is shown.
const SHOWN: usize = 50;

/// The most pictures that go with a job.
const PICTURES: usize = 10;

/// Whether each of a job's `attachments`, in the order they were added, goes with it as a picture: when
/// the model sees pictures, those an agent is shown too, up to `PICTURES` (docs/ai.md#附件). As `unseen`
/// in apps/web/src/attach.ts; both are checked against fixtures/ai-pictures.json.
fn seen(attachments: &[&Attachment], vision: bool) -> Vec<bool> {
    let mut sent = 0;
    attachments
        .iter()
        .map(|a| {
            let seen = vision && showable_image(a) && sent < PICTURES;
            sent += usize::from(seen);
            seen
        })
        .collect()
}

/// The instructions. `examples/ai_eval.rs` tries others in their place (docs/ai.md#评测).
pub const SYSTEM: &str = r#"你负责把用户随手写下的一段话整理成待办事项。输入是一个 JSON 对象：
- local_time：用户提交时的本地时间；
- folder：这些事项要放进的文件夹；
- open_tasks：这个文件夹里已有的未完成事项，number 是编号，after 是它的前置事项的编号，due、due_after 同下面输出的写法；
- text：用户写的原文。

只输出一个 json 对象，不要有任何别的文字，格式如下：
{"tasks": [{"ref": "a", "title": "…", "description": "…", "due": "2026-11-20 18:00", "remind": 30, "after": [], "before": []}]}

字段：
- title：必填，一行，简短（最好不超过 30 个字），以动作开头，如“写开题报告初稿”。原文没说是什么动作的，不要猜，保留原文的说法。
- description：选填，Markdown，只写标题装不下的信息。
- done：只有原文明确是记一件已经做完的事时才写（如“记一下昨天开了组会”），照 local_time 换算完成的公历日期，写 "YYYY-MM-DD"，不要带几点；不能晚于当地今天。明确做完但没说是哪天的写 true，待做的不写。这是完成时间，不是 due 或 start；没有待做事项也要把这件事列出来。
- start：选填，开始时间，"YYYY-MM-DD" 或 "YYYY-MM-DD HH:MM"，按农历说的见第 8 条：没到时事项等着，不显示在待办里。不能晚于 due。
- due：选填，截止时间，"YYYY-MM-DD" 或 "YYYY-MM-DD HH:MM"，按农历说的见第 8 条。
- due_after：选填，代替 due：前置事项（after）都完成后多久以内截止，"30d"（天）、"2w"（周）或 "3d 18:00"，从前置都完成的那天起算，"0d" 是当天。
- remind：只有原文明确要求提醒时才写。提前多久写 "30m"、"2h"、"1d"、"1w"（提前 30 分钟、2 小时、1 天、1 周）或分钟数，0 表示截止时；提前几分钟、几小时的要有带具体时间的 due 或 due_after，只有日期的 due 上 "1d" 这类整天的是提前那么多天的 09:00；说了在几点提醒的写几点："09:00" 是截止那天的几点，"1d 20:00" 是提前 1 天的 20:00，只有日期的 due 也能用。按开始时间提醒的前面加 start："start 10m" 是开始前 10 分钟，"start 0" 是开始时，"start 09:00" 是开始那天的 9 点，要有 start。另有截止、又要在别的某个时刻提醒一次的，写那个时刻，如 "2026-12-15 10:00"。几个提醒写成列表，如 ["30m", "1d"]；原文明确说不用提醒的写 []。
- repeat：写了的事项每次做完，会带着下一次的截止时间回到待办里，所以只有原文明确说了这件事每个周期都要人再做一次时才写，做的可以是用户自己，也可以是原文说的别人，如“每周五交周报”“每个工作日打卡”“每月 1 号还信用卡”。写法："1d" 每天、"2w" 每两周、"3m" 每三个月、"1y" 每年，"workdays" 每个工作日（周一到周五，跳过放假、算上补班的周末）、"1w:1,2,3,4,5" 每周一到周五（放假也照常）、"1w:1,3,5" 每周一三五，"1m:1,15" 每月 1 号和 15 号、"1m:31" 每月最后一天；按农历的见第 8 条。写了 repeat 就要写 due，见第 7 条；周期已经由 repeat 表示，不再写进标题和描述。先问自己：这一项做完一次以后，到下个周期还要人再做一遍吗？不用的就不写，即使原文里有“每天”“每周”：
   - “有空就”“多”这类没说周期的；
   - 只是在说情况的，如“这阵子每周都出差”；
   - 做一次就完、之后由程序（脚本、定时任务）自动按周期运行的，如“配一个每天凌晨跑的定时任务”，这时周期写进标题或描述。
- repeat_until：选填，只能和 repeat 一起写：重复到哪天为止，"YYYY-MM-DD"，含这天。原文说了到哪天为止的才写，如“每天健身打卡，坚持到 1 月 31 日”；说的是到某件事之前、又给了那件事的日子的，写那天的前一天，如“1 月 10 日答辩，答辩前每天练一遍讲稿”写 1 月 9 日。没给日子的不写。repeat_until 的日子不再写进标题和描述；到之前的那件事本身（如答辩）和它的日子是另一条信息，照样要留下：写进描述，或另建一个事项。
- 选填的字段没有内容时省略，不要写空字符串。
- ref：必填，这次结果里每个事项的临时名称，依次写 "a"、"b"、"c"……，供 after、before 引用。
- after：选填，前置事项，它们完成后才能做这一项：这次结果里的 ref，或 open_tasks 里的编号（数字）。
- before：选填，后续事项，这一项完成后才能做它们；取值同 after。

整理时：
1. 信息一点不丢：原文里的每个要求、细节、理由、背景、疑问和不确定之处，都要体现在某个事项里：标题、描述，或 due、due_after、start、remind、repeat 这些字段；不要编造原文没有的内容。
2. 标题写清这件事本身。原文提到的每个对象都要出现在标题里（拆成几条时，各自出现在对应事项的标题里），只调整说法，不删减。
3. 描述用简洁的书面语，自己重新组织，不照搬原话：
   - 去掉语气词、口头填充（“就是”“那个”“然后”）、重复和说到一半的改口，改口只留改过的说法；
   - 不写第一人称的口头叙述（“我觉得”“我发现”“我建议”），直接写事实和要求，如“我觉得按钮颜色有点淡”写成“按钮颜色偏淡”；
   - 原文拿不准的地方照样写明，如“好像只能搜到标题”写成“目前似乎只能搜索标题，待确认”；
   - 有几点时写成列表，每点一句；
   - 不重复标题已经说了的内容。标题已经说全了（原文很短时通常如此），就不写描述。
4. 代码、命令、链接、文件路径、数字和数据，以及引号里要照着用的原话，原样放进描述，一个字都不改（已经写进 due、due_after、start、remind、repeat 的日期、时间和周期不用再写）；代码和命令用代码块包起来。写给 AI 或 agent 的提示词和别的内容一样，改成简洁的书面语，每一条要求都要留下。
5. 原文说的是几件能分开做的事，就拆成几条，有先后的用 after 或 before 连起来；说的是一件事，就只输出一条，不要硬拆。几条是为了同一个目的的（如“把 CI 修好：先……再……”），这个目的写进每条的标题或描述。
6. 逐条看 open_tasks：这件事要用到某个已有事项的成果，或要等它做完才能开始，就把那个编号写进这件事的 after，例如“写官网文档”要等“建好官网”；已有事项要等这件事做完才能开始的，把它的编号写进 before，例如“开存款证明”做完才能“提交签证材料”。只是主题相近、没有先后的，不连。
7. 只有原文明确说了时间才写 due，“明天下午三点”“周五前”这类按 local_time 换算成具体日期，没说年份的按上下文定，看不出的取 local_time 以后最近的那一天；说了几点才带上时间；只说了“上午”“晚上”“下班前”“睡前”这类、没说几点的只写日期，不要替用户定一个时间，写了 repeat 的也一样。原文没说要提醒就不写 remind。只有原文明确说了某个时候以后才能做（“1 号开始领”“下周一起报名”“晚上 8 点开抢”），或者要等到某个时候某件事才发生（“等 11 月底训练营开营”）才写 start，同样换算；这个日子是开始，不是截止，原文没另说什么时候以前做完就不写 due。原文没另说截止、又要在这个时候或之前提醒的除外（“晚上 8 点开抢，提前 10 分钟提醒我”）：那时就要去做，把这个时候写成 due 并写 remind，不写 start。另说了截止、又要在开始时或开始前提醒的（“12 月 1 号起才能用，10 号前用掉，到时候提醒我”），写按开始算的 remind，如 "start 0"。只说了要在某个时刻提醒做一件事的（“明天下午 3 点提醒我取快递”），这个时刻就是截止，写 due 和 remind 0。写了 repeat 的，due 是第一次，按 local_time 换算：从开始那天（原文说的，如“从明天起”“下周开始”；没说就是今天）起最近的一次，如“每周一”是开始那天或以后的第一个周一，没说周几、几号的（“每天”“每 30 天”）就是开始那天。第一次不能已经过去：算出的是今天、而原文说的几点已经过了的（如 20:00 时说“每天早上八点”），按重复推到下一次。开始那天已由 due 表示，不另写 start。原文的期限是从另一件事做完时算的（“拿到发票后 30 天内报销”“签完合同 3 天内付款”“面试后 24 小时内发感谢信”），这件事要写 after，等的那件事是前置：已有的写编号，没有的另建一个（如“拿到发票”“面试”），再写 due_after，不写 due；没有前置的事项不能写 due_after；不足一天的写 "0d" 或 "1d"。期限是从现在算的（“三天后”“一周内”“周五前”），照样换算成日期写 due，即使这一项有前置。
8. 原文按农历说的日子（“腊月二十三”“农历八月十五”，春节、元宵、端午、中秋、除夕这些节日也是），due 和 start 不要自己换算成公历，写农历日期，如 "农历八月十五"，说了几点的带上，如 "农历腊月廿三 15:00"，没说几点的（如只说“晚上”）只写日期，同第 7 条；会换算成今天起最近的那一天；除夕写 "农历腊月三十"（腊月只有 29 天的年份就是廿九）；只说几号、没说月份的写 "农历初一"，是今天起的第一个初一。原文说了更晚的年份、或从以后某天才开始的，按 local_time 最后写的今天的农历日子自己换算成公历日期。按农历重复的在 repeat 的单位前加 l："1ly" 每年农历的这一天（due 的农历月和日），如每年农历八月十五过生日写 due "农历八月十五"、repeat "1ly"；"1lm:1,15" 农历每月初一和十五，这时 due 写其中一天，如 "农历初一"，会换算成这几天里最先到的那天。
9. 用原文的语言写，不要翻译：原文是英文，标题和描述也用英文。

例子（只示范写法，不要套用其中的内容）：
- 原文“嗯那个，就是周报模板我觉得太长了，好多栏目其实根本没人填，可以删掉一些吧，比如风险那栏，下周计划好像也可以合并到本周进展里”，输出：
  {"tasks": [{"ref": "a", "title": "精简周报模板", "description": "模板过长，不少栏目无人填写，可以删减：\n- 删去“风险”一栏\n- “下周计划”或可并入“本周进展”"}]}
- 原文“报销单和发票整理”，输出：
  {"tasks": [{"ref": "a", "title": "整理报销单和发票"}]}
- 原文“先订好酒店，再按酒店的位置规划每天的行程”，输出：
  {"tasks": [{"ref": "a", "title": "订酒店"}, {"ref": "b", "title": "按酒店位置规划每天的行程", "after": ["a"]}]}
- local_time 是 2026-11-04 16:30 星期三，原文“每周五下午五点前交周报，另外让运维把日志清理改成每天自动跑”，输出：
  {"tasks": [{"ref": "a", "title": "交周报", "due": "2026-11-06 17:00", "repeat": "1w"}, {"ref": "b", "title": "让运维把日志清理改成每天自动运行"}]}
- 原文“供应商的报价单发来以后 3 天内给他们答复”，输出：
  {"tasks": [{"ref": "a", "title": "收到供应商的报价单"}, {"ref": "b", "title": "答复供应商", "after": ["a"], "due_after": "3d"}]}"#;

/// Added to the instructions in a shared folder, where the text may say who is to do what.
pub const ASSIGN: &str = r#"这个文件夹是共享的，输入里还有 people：共享人和成员，username 是用户名，name 是显示名，me 为 true 的是写这段话的用户。原文说了某件事由谁来做（“让小王去问报价”“我来订酒店”），就在那个事项上写 assign：负责人的 username 列表，如 "assign": ["wang"]；没说的不写，不要猜。"#;

/// Added to the instructions when the job has attachments.
pub const ATTACHMENTS: &str = r#"输入里还有 attachments：用户随原文附上的文件，ref 是附件的编号，name 是名称，type 是类型；image 为 true 的是图片，附在这条消息后面，每张前面写明了是第几个附件。看得到的图片里的信息和原文一样，要整理进事项；看不清的不要猜。每个事项可以写 attachments：放在这一项下的附件的 ref 列表，如 "attachments": ["1"]。每个附件只放在一项下，没写的放在第一项下。"#;

/// Added to the instructions, of organizing and of tidying, when the user has notes along the way to
/// the folder (docs/ai.md#说明).
pub const NOTES: &str = "输入里还有 notes：用户写给你的长期要求，按文件夹从外到内排列，folder 是它所属的文件夹。整理时照着做，它们和上面的规则（如用原文的语言写、已经清楚的不要改）不一致时，以它们为准：已经清楚、但不合它们的要求的事项也要改。内层文件夹的要求和外层的不一致时，以内层的为准。它们只管怎么做，不扩大这次要做的范围：这次没要求改的地方，不因为它们去改；也不改变上面规定的输出格式和字段。";

pub(super) fn due_text(d: &Due) -> String {
    match &d.at {
        Some(at) => format!("{} {}", d.date.as_str(), at.time.as_str()),
        None => d.date.as_str().to_owned(),
    }
}

const CALENDAR: &str = r#"输入若带 day，是用户在日历选中的日期、星期和可选的钟点，例如 "2026-11-30 星期一 13:00"。它不是今天。只用它补齐待做事项的截止时间：
- 原文说了截止日期，按原文；“明天”“周五”“三天后”等仍从 local_time 算。没说截止日期，用 day 的日期，即使原文没提到时间。
- 原文说了几点，用原文的钟点；没说几点而 day 带钟点，用 day 的钟点；两处都没给钟点，只写日期。例如选了周一 13:00，只说“下午三点寄件”，截止在周一 15:00。
- 不用 day 改变开始、重复、提醒或完成时间的规则。已有 due_after 或由重复规则算出的首次截止日期，不改用 day 的日期。不添加早于开始时间的默认截止。"#;

/// What a chat shows: the tasks, by the numbers it shows them by, the only ones its tasks may follow or
/// come before (by id, as a task's number changes if it moves to another owner's while the model
/// answers); and the job's attachments, the first by the ref "1".
pub struct Shown {
    pub tasks: HashMap<i64, TaskId>,
    pub attachments: Vec<AttachmentId>,
}

/// A picture that goes with a chat, still to be read from its file.
pub struct PictureFile {
    caption: String,
    id: AttachmentId,
    size: FileSize,
    media_type: String,
}

impl PictureFile {
    pub fn read(self, files: &Files) -> std::io::Result<Picture> {
        let data = STANDARD.encode(files.read(self.id, self.size)?);
        Ok(Picture { caption: self.caption, url: format!("data:{};base64,{data}", self.media_type) })
    }
}

/// The chat for `job`, with the user's `notes` along the way to its folder and its `attachments` in
/// the order added, the pictures among them too when the model has `vision`; what it shows, and the
/// pictures to go with it.
pub fn prompt(
    job: &AiJob,
    board: &Board,
    notes: Vec<Value>,
    attachments: &[&Attachment],
    vision: bool,
) -> (Prompt, Shown, Vec<PictureFile>) {
    let open: Vec<&Task> = board.tasks_in(job.folder_id).into_iter().filter(|t| t.done_at.is_none()).take(SHOWN).collect();
    let shown = Shown { tasks: open.iter().map(|t| (t.number, t.id)).collect(), attachments: attachments.iter().map(|a| a.id).collect() };
    let tasks: Vec<Value> = open
        .iter()
        .map(|t| {
            let mut v = json!({ "number": t.number, "title": t.title.as_str() });
            if let Some(d) = &t.due {
                v["due"] = due_text(d).into();
            }
            if let Some(a) = &t.due_after {
                v["due_after"] = due_after_text(a).into();
            }
            let after: Vec<i64> = board.before(t).iter().map(|b| b.number).filter(|n| shown.tasks.contains_key(n)).collect();
            if !after.is_empty() {
                v["after"] = after.into();
            }
            v
        })
        .collect();
    let mut input = json!({
        "local_time": job.local_time.as_str(),
        "folder": board.path(job.folder_id),
        "open_tasks": tasks,
        "text": job.text.as_str(),
    });
    if let Some(day) = &job.day {
        // The Unix epoch was a Thursday.
        let weekday = (day.days_after(&Date::utc(0)) + 3).rem_euclid(7) as usize;
        let weekday = ["一", "二", "三", "四", "五", "六", "日"][weekday];
        let mut picked = format!("{} 星期{weekday}", day.as_str());
        if let Some(time) = &job.time {
            picked.push(' ');
            picked.push_str(time.as_str());
        }
        input["day"] = picked.into();
    }
    if !notes.is_empty() {
        input["notes"] = notes.into();
    }
    let mut pictures = Vec::new();
    let mut listed = Vec::new();
    for ((i, a), image) in attachments.iter().enumerate().zip(seen(attachments, vision)) {
        let r = (i + 1).to_string();
        let media_type = a.media_type.as_str();
        if image {
            pictures.push(PictureFile {
                caption: format!("附件 {r}：{}", a.name.as_str()),
                id: a.id,
                size: a.size,
                media_type: media_type.to_owned(),
            });
        }
        listed.push(json!({ "ref": r, "name": a.name.as_str(), "type": media_type, "image": image }));
    }
    if !listed.is_empty() {
        input["attachments"] = listed.into();
    }
    let Some(share) = board.share(job.folder_id) else { return (chat(SYSTEM, &input), shown, pictures) };
    let people: Vec<Value> = std::iter::once(&share.owner)
        .chain(&share.members)
        .filter_map(|id| board.person(id))
        .map(|p| {
            let mut v = json!({ "username": p.username, "name": p.display_name });
            if p.id == board.me() {
                v["me"] = true.into();
            }
            v
        })
        .collect();
    input["people"] = people.into();
    (chat(SYSTEM, &input), shown, pictures)
}

/// The chat that gives the model `input`, a job and its folder as `prompt` makes them, with the
/// instructions `system` and those for what else the input has: the people of a shared folder, the
/// user's notes, the job's attachments and the calendar day. Without pictures. The local time has
/// the day in the lunar calendar after it, `2026-11-26 10:00 星期四 农历十月十八`: models can't work it out.
pub fn chat(system: &str, input: &Value) -> Prompt {
    let mut system = system.to_owned();
    for (key, more) in [("people", ASSIGN), ("notes", NOTES), ("attachments", ATTACHMENTS), ("day", CALENDAR)] {
        if input.get(key).is_some() {
            system = format!("{system}\n\n{more}");
        }
    }
    let mut input = input.clone();
    if let Some(local) = input["local_time"].as_str()
        && let Some(lunar) = local.split_whitespace().next().and_then(|d| Date::try_from(d.to_owned()).ok()?.lunar())
    {
        input["local_time"] = format!("{local} 农历{lunar}").into();
    }
    Prompt { system, user: serde_json::to_string_pretty(&input).expect("JSON values serialize"), pictures: Vec::new() }
}

/// A minimal chat that answers in JSON, to see whether a config works.
pub fn test_prompt() -> Prompt {
    Prompt { system: r#"只输出这个 json 对象：{"ok": true}"#.into(), user: "测试".into(), pictures: Vec::new() }
}

/// A 16×16 PNG all red.
const RED: &str =
    "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABAAAAAQCAIAAACQkWg2AAAAFklEQVR42mP4z8BAEmIY1TCqYfhqAACQ+f8B8u7oVwAAAABJRU5ErkJggg==";

/// A chat asking the colour of a red picture, to see whether the model sees pictures: whether it does
/// is `sees_red` of the answer.
pub fn vision_prompt() -> Prompt {
    Prompt {
        system: r#"看图回答图片是什么颜色，只输出这个 json 对象：{"color": "颜色"}"#.into(),
        user: "这张图片是什么颜色？".into(),
        pictures: vec![Picture { caption: "图片：".into(), url: RED.into() }],
    }
}

pub fn sees_red(answer: &str) -> bool {
    answer.contains('红') || answer.to_lowercase().contains("red")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    tasks: Vec<Value>,
}

pub struct OrganizedTask {
    fields: PlanTask,
    done: Option<CompletionMark>,
    attachments: Vec<AttachmentId>,
}

pub struct OrganizedPlan {
    pub ops: Vec<Op>,
    first: TaskId,
    holders: HashMap<AttachmentId, TaskId>,
}

impl OrganizedPlan {
    pub(super) fn holder(&self, attachment: AttachmentId) -> TaskId {
        self.holders.get(&attachment).copied().unwrap_or(self.first)
    }
}

pub fn plan(
    b: &Board,
    folder: Option<FolderId>,
    answer: &[OrganizedTask],
    shown: &Shown,
    created_at: i64,
    reminders: Option<&DefaultReminders>,
) -> Result<OrganizedPlan, ApiError> {
    let mut planned = Vec::new();
    let mut holders = HashMap::new();
    for item in answer {
        let mut task = crate::tools::PlannedTask::from(item.fields.clone());
        if let Some(r) = reminders
            && task.fields.remind.is_none()
        {
            let t = &task.fields;
            let timed = t.due.as_ref().map(|d| d.time.is_some()).or(t.due_after.as_ref().map(|a| a.time.is_some()));
            task.fields.remind = timed.map(|timed| RemindText { due: r.for_due(timed), ..RemindText::default() });
        }
        match &item.done {
            None => {}
            Some(CompletionMark::State(state)) => task.done = *state,
            Some(CompletionMark::Date(date)) => {
                let days = date.days_after(&b.now().today);
                if days > 0 {
                    return Err(ApiError::invalid("完成日期不能晚于今天", Some("done".into())));
                }
                task.done = true;
                task.done_at = Some(created_at + days * 86_400_000);
            }
        }
        for &attachment in &item.attachments {
            holders.entry(attachment).or_insert(task.id);
        }
        planned.push(task);
    }
    let first = planned[0].id;
    let ops = crate::tools::plan_into(b, folder, planned, &shown.tasks)?;
    Ok(OrganizedPlan { ops, first, holders })
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

/// The tasks in the model's answer, linked only to each other and to tasks in `shown`; with the refs of
/// the attachments each lists.
pub fn parse(answer: &str, shown: &Shown) -> Result<Vec<OrganizedTask>, String> {
    let v: Value = serde_json::from_str(unwrap(answer)).map_err(|_| "模型返回的不是合法的 JSON".to_owned())?;
    let invalid = |e: ApiError| format!("模型返回的结果不符合格式：{}", e.describe());
    let Answer { tasks } = from_value(v).map_err(invalid)?;
    if tasks.is_empty() {
        return Err("模型没有整理出事项".into());
    }
    let shown_attachments: HashMap<_, _> = shown.attachments.iter().enumerate().map(|(i, &id)| ((i + 1).to_string(), id)).collect();
    let mut parsed = Vec::new();
    for mut fields in tasks {
        let Some(values) = fields.as_object_mut() else { return Err("模型返回的事项不是对象".into()) };
        // Models may leave an empty field for one they have nothing for, but the title has to be there.
        values.retain(|k, f| k == "title" || !(f.is_null() || f.as_str().is_some_and(|s| s.trim().is_empty())));
        let done = values.remove("done").map(from_value).transpose().map_err(invalid)?;
        super::check_completion_mark(&done).map_err(invalid)?;
        let attachments = attachments(values.remove("attachments"), &shown_attachments);
        let t: PlanTask = from_value(fields).map_err(invalid)?;
        if t.folder.is_some() {
            return Err("模型返回的结果指定了文件夹".into());
        }
        for r in t.after.iter().chain(&t.before) {
            let n = match r {
                PlanRef::Number(n) => Some(*n),
                PlanRef::Name(s) => match task_ref(s) {
                    Some((None, n)) => Some(n),
                    Some((Some(_), _)) => return Err(format!("结果引用了不在这个文件夹里的事项 {s}")),
                    None => None,
                },
            };
            if let Some(n) = n.filter(|n| !shown.tasks.contains_key(n)) {
                return Err(format!("结果引用了不在这个文件夹里的事项 #{n}"));
            }
        }
        parsed.push(OrganizedTask { fields: t, done, attachments });
    }
    Ok(parsed)
}

/// The refs in a task's `attachments`, which models write as strings or numbers; nothing else is one.
fn attachments(attachments: Option<Value>, shown: &HashMap<String, AttachmentId>) -> Vec<AttachmentId> {
    let Some(Value::Array(refs)) = attachments else { return Vec::new() };
    refs.into_iter()
        .filter_map(|r| match r {
            Value::String(s) => shown.get(s.trim()).copied(),
            Value::Number(n) => shown.get(&n.to_string()).copied(),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_go_with_a_job_as_the_fixtures_say() {
        #[derive(serde::Deserialize)]
        struct Case {
            name: String,
            vision: bool,
            attachments: Vec<Value>,
            seen: Vec<bool>,
        }
        let cases: Vec<Case> = serde_json::from_str(include_str!("../../../../fixtures/ai-pictures.json")).unwrap();
        for c in cases {
            let job = uuid::Uuid::now_v7();
            let attachment = |a: &Value| -> Attachment {
                let (id, kind, size) = (uuid::Uuid::now_v7(), &a["type"], &a["size"]);
                serde_json::from_value(json!({ "id": id, "job_id": job, "name": "图", "type": kind, "size": size, "created_at": 0 }))
                    .unwrap()
            };
            let attachments: Vec<Attachment> = c.attachments.iter().map(attachment).collect();
            assert_eq!(seen(&attachments.iter().collect::<Vec<_>>(), c.vision), c.seen, "{}", c.name);
        }
    }

    #[test]
    fn takes_json_out_of_thinking_and_fences() {
        assert_eq!(unwrap("<think>先想想</think>\n```json\n{\"tasks\": []}\n```"), "{\"tasks\": []}");
        assert_eq!(unwrap("```\n{}\n```"), "{}");
        assert_eq!(unwrap(" {} "), "{}");
    }

    #[test]
    fn checks_the_answer() {
        let shown = Shown { tasks: HashMap::from([(12, TaskId(uuid::Uuid::nil()))]), attachments: Vec::new() };
        let ok = r##"{"tasks": [{"ref": "a", "title": "写初稿"}, {"title": "改稿", "after": ["a", 12, "#12"]}]}"##;
        assert_eq!(parse(ok, &shown).unwrap().len(), 2);
        assert_eq!(parse("好的", &shown).err().unwrap(), "模型返回的不是合法的 JSON");
        assert_eq!(parse(r#"{"tasks": []}"#, &shown).err().unwrap(), "模型没有整理出事项");
        assert!(parse(r#"{"tasks": [{"title": " "}]}"#, &shown).err().unwrap().starts_with("模型返回的结果不符合格式：标题不能为空"));
        let empty = parse(r#"{"tasks": [{"title": "交初稿", "due": "", "description": null, "ref": " "}]}"#, &shown).unwrap();
        assert!(empty[0].fields.due.is_none() && empty[0].fields.description.is_none() && empty[0].fields.name.is_none());
        assert!(parse(r#"{"tasks": [{"title": "a", "note": "b"}]}"#, &shown).is_err());
        assert_eq!(parse(r#"{"tasks": [{"title": "a", "folder": "/x"}]}"#, &shown).err().unwrap(), "模型返回的结果指定了文件夹");
        assert_eq!(parse(r#"{"tasks": [{"title": "a", "before": [7]}]}"#, &shown).err().unwrap(), "结果引用了不在这个文件夹里的事项 #7");
    }

    #[test]
    fn a_task_can_be_organized_as_done_at_the_epoch_but_not_before() {
        let shown = Shown { tasks: HashMap::new(), attachments: Vec::new() };
        let answer = |date| json!({"tasks": [{"title": "开会", "done": date}]}).to_string();
        assert_eq!(parse(&answer("1969-12-31"), &shown).err().unwrap(), "模型返回的结果不符合格式：完成日期不能早于 1970-01-01（done）");
        let answer = parse(&answer("1970-01-01"), &shown).unwrap();
        let snap = nexttodo_core::board::Snapshot::default();
        let now = 86_400_000;
        let b = Board::new(&snap, nexttodo_core::board::Now::utc(now), "me");
        let plan = plan(&b, None, &answer, &shown, now, None).unwrap();
        assert!(matches!(plan.ops.as_slice(), [Op::TaskCreate { done: true, done_at: Some(0), .. }]));
    }

    #[test]
    fn attachments_go_under_the_first_task_listing_them_else_the_first() {
        let attachments = std::array::from_fn::<_, 4, _>(|_| AttachmentId(uuid::Uuid::now_v7()));
        let shown = Shown { tasks: HashMap::new(), attachments: attachments[..3].to_vec() };
        let answer = r#"{"tasks": [{"title": "a", "attachments": ["3", 9]}, {"title": "b", "done": "2026-11-07", "attachments": [2, " 3", {}]}, {"title": "c", "attachments": "1"}]}"#;
        let answer = parse(answer, &shown).unwrap();
        let snap = nexttodo_core::board::Snapshot::default();
        let now = 1_794_096_000_000;
        let b = Board::new(&snap, nexttodo_core::board::Now::utc(now), "me");
        let plan = plan(&b, None, &answer, &shown, now, None).unwrap();
        let [
            Op::TaskCreate { id: first, done: false, .. },
            Op::TaskCreate { id: second, done: true, done_at: Some(at), .. },
            Op::TaskCreate { done: false, .. },
        ] = plan.ops.as_slice()
        else {
            panic!("{:?}", plan.ops)
        };
        assert_eq!(attachments.map(|a| plan.holder(a)), [*first, *second, *first, *first]);
        assert_eq!(*at, now - 86_400_000);
    }
}
