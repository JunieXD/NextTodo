//! Tool arguments. Descriptions are written for the agents that read the tool list.

use nexttodo_core::board::Now;
use nexttodo_core::lunar;
use nexttodo_core::model::{
    Accent, AttachmentId, Date, Description, DueAfter, Every, FileName, FolderColor, FolderIcon, KeepDone, MediaType, Monthdays, Period,
    Remind, RemindBefore, RemindOn, RepeatFrom, Time, Title, Weekdays,
};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};

use crate::mutate::check_completion_date;

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoArgs {}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListArgs {
    #[schemars(description = "文件夹路径，如 /工作/网站改版；默认根目录 /")]
    pub folder: Option<String>,
    #[serde(default)]
    #[schemars(description = "同时列出已完成的事项")]
    pub all: bool,
    #[schemars(description = "最多列出多少项，默认 50，最多 500", range(min = 1, max = 500))]
    pub limit: Option<u32>,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NextArgs {
    #[schemars(description = "只列这些文件夹（含子文件夹）里的，路径如 /工作、@alice/装修；有 / 时就是全部")]
    pub folders: Option<Vec<String>>,
    #[schemars(description = "最多列出多少项，默认 50，最多 500", range(min = 1, max = 500))]
    pub limit: Option<u32>,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgendaArgs {
    #[schemars(description = "列今天起几天，默认 7，最多 90", range(min = 1, max = 90))]
    pub days: Option<u32>,
    #[schemars(description = "只列这些文件夹（含子文件夹）里的，路径如 /工作、@alice/装修；有 / 时就是全部")]
    pub folders: Option<Vec<String>>,
}

/// A task by number: `42` or `"#42"`, `"@alice#42"` in a folder someone else shares, which is
/// alice's #42.
#[derive(Clone, Deserialize, JsonSchema)]
#[serde(try_from = "TaskRefArg")]
pub struct TaskRef {
    pub user: Option<String>,
    pub number: i64,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
#[schemars(description = "事项编号：42 或 \"#42\"；他人共享给我的文件夹里的事项写成 \"@alice#42\"")]
enum TaskRefArg {
    Number(i64),
    Text(String),
}

impl TryFrom<TaskRefArg> for TaskRef {
    type Error = String;

    fn try_from(r: TaskRefArg) -> Result<Self, String> {
        match r {
            TaskRefArg::Number(number) => Ok(TaskRef { user: None, number }),
            TaskRefArg::Text(s) => {
                task_ref(&s).map(|(user, number)| TaskRef { user: user.map(str::to_owned), number }).ok_or(format!("“{s}”不是事项编号"))
            }
        }
    }
}

impl std::fmt::Display for TaskRef {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match &self.user {
            Some(user) => write!(f, "@{user}#{}", self.number),
            None => write!(f, "#{}", self.number),
        }
    }
}

/// A task number written as text: `"42"` or `"#42"`, or `"@alice#42"` with whose it is.
pub fn task_ref(s: &str) -> Option<(Option<&str>, i64)> {
    let s = s.trim();
    let (user, number) = match s.strip_prefix('@') {
        Some(rest) => rest.split_once('#').map(|(user, n)| (Some(user.trim()), n))?,
        None => (None, s.strip_prefix('#').unwrap_or(s)),
    };
    Some((user, number.trim().parse().ok()?))
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShowArgs {
    #[schemars(description = "事项编号")]
    pub task: TaskRef,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchArgs {
    #[schemars(description = "关键词")]
    pub query: String,
    #[schemars(description = "最多列出多少项，默认 50，最多 500", range(min = 1, max = 500))]
    pub limit: Option<u32>,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphArgs {
    #[schemars(description = "文件夹路径，包含它的子文件夹；默认根目录 /，即全部事项")]
    pub folder: Option<String>,
    #[serde(default)]
    #[schemars(description = "同时列出已完成的事项")]
    pub all: bool,
    #[serde(default)]
    #[schemars(description = "输出 Mermaid 流程图")]
    pub mermaid: bool,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LogArgs {
    #[schemars(description = "条数，默认 10，最多 100", range(min = 1, max = 100))]
    pub limit: Option<u32>,
    #[schemars(description = "批次号：只列在它之前的，用来往前翻")]
    pub before: Option<i64>,
    #[schemars(description = "只列删除了事项、文件夹或附件的批次")]
    #[serde(default)]
    pub deleted: bool,
    #[schemars(description = "批次号：只看这一个批次，每类变化列出全部事项；给了它其他参数不起作用")]
    pub batch: Option<i64>,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddArgs {
    #[schemars(with = "String", description = "标题，以动作表述")]
    pub title: Title,
    #[schemars(description = "文件夹路径；默认根目录 /；不存在时一起新建")]
    pub folder: Option<String>,
    #[schemars(with = "Option<String>", description = "描述，Markdown；写清怎样算完成")]
    pub description: Option<Description>,
    #[schemars(
        with = "Option<String>",
        description = "开始时间：2026-11-14 或 2026-11-14 09:30，农历的写法同 due，没到时事项等着，不进“接下来”；不能晚于截止"
    )]
    pub start: Option<WhenText>,
    #[schemars(
        with = "Option<String>",
        description = "截止时间：2026-11-20 或 2026-11-20 18:00；按农历的写 农历八月十五、农历腊月廿三 18:00，是今天起最近的那一天（小月没有三十的取最后一天，除夕写农历腊月三十），不写月份的 农历初一 是今天起的第一个初一；配上按农历每个月几号的重复（如 1lm:1,15）时，从写的那个月起（不写月份或写的是本月的，从现在起）、不早于开始时间，取这几天里最先到的那天，要定下第一次的写公历日期"
    )]
    pub due: Option<WhenText>,
    #[schemars(
        with = "Option<String>",
        description = "代替 due：前置（after）都完成后多久以内截止，30d（天）、2w（周）或 3d 18:00，从前置都完成的那天起算，0d 是当天。用在期限从另一件事做完时算的事项上，如拿到发票后 30 天内报销；没有前置时就从今天算。remind 照样给，不能和 repeat 一起用"
    )]
    pub due_after: Option<DueAfterText>,
    #[schemars(
        with = "Option<RemindInput>",
        description = "提醒，一个或几个（列表）：提前多久写 \"30m\"、\"2h\"、\"1d\"、\"1w\" 或分钟数，0 是截止时，要有带具体时间的截止（只有日期的截止上，整天的如 \"1d\"、0 是提前那么多天的 09:00）；\"09:00\" 是截止那天的几点，\"1d 20:00\" 是提前 1 天的 20:00，只有日期的截止也能设；按开始时间算的前面加 start，如 \"start 30m\"、\"start 0\"，要有开始时间；某个时刻提醒一次的写日期和几点，如 \"2026-12-15 10:00\"，没有截止也能设。如 [\"30m\", \"1d 20:00\"]"
    )]
    pub remind: Option<RemindText>,
    #[schemars(
        with = "Option<String>",
        description = "重复：间隔数加单位，1d 每天、2w 每两周、3m 每三个月、1y 每年；按周的可以在冒号后写周几（1 周一到 7 周日），1w:1,2,3,4,5 每周一到周五、1w:1,3,5 每周一三五；workdays 每个法定工作日（周一到周五，跳过放假、算上补班的周末），用户说每个工作日的一般是它；按月的可以在冒号后写几号（1–31，没有这天的月份取最后一天），1m:1,15 每月 1 号和 15 号、1m:31 每月最后一天；按农历的在单位前加 l，可以在冒号后写几号（1–30，初一到三十），不写的是截止日期的农历日子：1ly 每年农历的这一天、1lm:1,15 农历每月初一和十五；要有截止时间。完成时顺延截止时间，开始时间跟着顺延，不会真的完成（repeat_until 前的最后一次除外）"
    )]
    pub repeat: Option<RepeatText>,
    #[schemars(with = "Option<String>", description = "下一次从哪天算：due 按截止日期（默认），done 从完成那天；只能和 repeat 一起给")]
    pub repeat_from: Option<RepeatFrom>,
    #[schemars(
        with = "Option<String>",
        description = "重复到哪天（YYYY-MM-DD，含这天），不给就一直重复；做完最后一次，事项就完成了。只能和 repeat 一起给"
    )]
    pub repeat_until: Option<Date>,
    #[serde(default)]
    #[schemars(description = "负责人的用户名：共享的文件夹里才能设，须是共享人或成员；不设就是谁都可以做")]
    pub assign: Vec<String>,
    #[serde(default)]
    #[schemars(description = "前置事项的编号：它们完成后才能做这一项")]
    pub after: Vec<TaskRef>,
    #[serde(default)]
    #[schemars(description = "后续事项的编号：这一项完成后才能做它们")]
    pub before: Vec<TaskRef>,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditArgs {
    #[schemars(description = "事项编号")]
    pub task: TaskRef,
    #[schemars(with = "Option<String>", description = "新标题")]
    pub title: Option<Title>,
    #[schemars(with = "Option<String>", description = "新描述，Markdown，整段替换；只改其中一段时用 replace 或 append")]
    pub description: Option<Description>,
    #[serde(default)]
    #[schemars(
        description = "在服务端当时的描述上依次替换：old 必须恰好出现一次，找不到时返回冲突，可能已被改过，先 show 再改。别处同时改的内容不受影响"
    )]
    pub replace: Vec<Replace>,
    #[schemars(description = "在描述末尾另起一行追加，在 replace 之后")]
    pub append: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(
        with = "Option<String>",
        description = "开始时间：2026-11-14 或 2026-11-14 09:30，农历的写法同 due，没到时事项等着；只写日期的去掉原来的几点；null 清除。不能晚于截止"
    )]
    pub start: Option<Option<WhenText>>,
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(
        with = "Option<String>",
        description = "截止时间：2026-11-20 或 2026-11-20 18:00；按农历的写 农历八月十五、农历腊月廿三 18:00，是今天起最近的那一天（小月没有三十的取最后一天，除夕写农历腊月三十），不写月份的 农历初一 是今天起的第一个初一；配上按农历每个月几号的重复（如 1lm:1,15）时，从写的那个月起（不写月份或写的是本月的，从现在起）、不早于开始时间，取这几天里最先到的那天，要定下第一次的写公历日期；null 清除。不给 remind 时保留原来的提醒，新时间不带具体时间的去掉提前多少分钟的"
    )]
    pub due: Option<Option<WhenText>>,
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(
        with = "Option<String>",
        description = "代替 due：前置都完成后多久以内截止，30d（天）、2w（周）或 3d 18:00，从前置都完成的那天起算，0d 是当天；前置已经都完成或没有前置的，从今天算；null 清除。提醒同 due 的规则保留，不能和 repeat 一起用"
    )]
    pub due_after: Option<Option<DueAfterText>>,
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(
        with = "Option<RemindInput>",
        description = "提醒，一个或几个（列表），整个换掉原来的：提前多久写 \"30m\"、\"2h\"、\"1d\"、\"1w\" 或分钟数，0 是截止时，要有带具体时间的截止（只有日期的截止上，整天的如 \"1d\"、0 是提前那么多天的 09:00）；\"09:00\" 是截止那天的几点，\"1d 20:00\" 是提前 1 天的 20:00，只有日期的截止也能设；按开始时间算的前面加 start，如 \"start 30m\"、\"start 0\"，要有开始时间；某个时刻提醒一次的写日期和几点，如 \"2026-12-15 10:00\"，没有截止也能设；null 或 [] 清除"
    )]
    pub remind: Option<Option<RemindText>>,
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(
        with = "Option<String>",
        description = "重复：间隔数加单位，1d 每天、2w 每两周、3m 每三个月、1y 每年；按周的可以在冒号后写周几（1 周一到 7 周日），1w:1,2,3,4,5 每周一到周五、1w:1,3,5 每周一三五；workdays 每个法定工作日（周一到周五，跳过放假、算上补班的周末），用户说每个工作日的一般是它；按月的可以在冒号后写几号（1–31，没有这天的月份取最后一天），1m:1,15 每月 1 号和 15 号、1m:31 每月最后一天；按农历的在单位前加 l，可以在冒号后写几号（1–30，初一到三十），不写的是截止日期的农历日子：1ly 每年农历的这一天、1lm:1,15 农历每月初一和十五；null 不再重复。只改截止时间时重复保留"
    )]
    pub repeat: Option<Option<RepeatText>>,
    #[schemars(with = "Option<String>", description = "下一次从哪天算：due 按截止日期（默认），done 从完成那天；只能和 repeat 一起给")]
    pub repeat_from: Option<RepeatFrom>,
    #[schemars(
        with = "Option<String>",
        description = "重复到哪天（YYYY-MM-DD，含这天），不给就一直重复；做完最后一次，事项就完成了。只能和 repeat 一起给"
    )]
    pub repeat_until: Option<Date>,
    #[serde(default)]
    #[schemars(description = "加为负责人的用户名，在服务端当时的负责人上加；须是共享人或成员")]
    pub assign: Vec<String>,
    #[serde(default)]
    #[schemars(description = "不再负责的用户名，从服务端当时的负责人里去掉")]
    pub unassign: Vec<String>,
    #[schemars(
        with = "Option<String>",
        description = "改已结束事项的完成时间，或还在重复的事项最晚完成的那条记录的日期。当地时间 YYYY-MM-DD 或 YYYY-MM-DD HH:MM，不早于 1970-01-01，不晚于现在；只给日期保留原来的钟点；未同时提供 today 和 time 时按 UTC 换算"
    )]
    pub done_at: Option<DoneWhen>,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Replace {
    #[schemars(description = "描述里现有的一段原文")]
    pub old: String,
    #[schemars(description = "换成的内容，可以为空")]
    pub new: String,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TasksArgs {
    #[schemars(description = "事项编号", length(min = 1))]
    pub tasks: Vec<TaskRef>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(try_from = "String")]
pub struct DoneWhen {
    pub date: Date,
    time: Option<Time>,
}

impl TryFrom<String> for DoneWhen {
    type Error = String;

    fn try_from(value: String) -> Result<Self, String> {
        let (date, time) = value.split_once(' ').map_or((value.as_str(), None), |(d, t)| (d, Some(t)));
        let invalid = |_| "完成时间请写成 YYYY-MM-DD 或 YYYY-MM-DD HH:MM".to_owned();
        let date = Date::try_from(date.to_owned()).map_err(invalid)?;
        check_completion_date(&date)?;
        Ok(Self { date, time: time.map(|t| Time::try_from(t.to_owned()).map_err(invalid)).transpose()? })
    }
}

pub(crate) fn local_millis(date: &Date, time: &Time) -> i64 {
    let (h, m) = time.as_str().split_once(':').expect("HH:MM");
    let minute = h.parse::<i64>().expect("hours") * 60 + m.parse::<i64>().expect("minutes");
    (date.days_after(&Date::utc(0)) * 1440 + minute) * 60_000
}

impl DoneWhen {
    pub(crate) fn at(&self, local: &Now, now: i64, previous: Option<i64>) -> Result<i64, String> {
        let utc = Now::utc(now);
        let local = if local.time.is_some() { local } else { &utc };
        let clock = local.time.as_ref().expect("a local or UTC time");
        let utc_minute = now.div_euclid(60_000) * 60_000;
        let offset = local_millis(&local.today, clock) - utc_minute;
        let at = match &self.time {
            Some(time) => local_millis(&self.date, time) - offset,
            None => {
                self.date.days_after(&Date::utc(0)) * 86_400_000
                    + (previous.unwrap_or(now).rem_euclid(86_400_000) + offset).rem_euclid(86_400_000)
                    - offset
            }
        };
        if at > now { Err("完成时间不能晚于现在".into()) } else { Ok(at) }
    }
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DoneArgs {
    #[schemars(description = "事项编号", length(min = 1))]
    pub tasks: Vec<TaskRef>,
    #[schemars(description = "重复的事项结束重复、直接完成，如读完一本书时；默认 false")]
    #[serde(default)]
    pub end_repeat: bool,
    #[schemars(
        with = "Option<String>",
        description = "补记完成：当地时间 YYYY-MM-DD 或 YYYY-MM-DD HH:MM，不早于 1970-01-01，不晚于现在；只给日期用现在的钟点，重复事项用 at 的日期而不是 today；未同时提供 today 和 time 时按 UTC 换算"
    )]
    pub at: Option<DoneWhen>,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DropArgs {
    #[schemars(description = "事项编号", length(min = 1))]
    pub tasks: Vec<TaskRef>,
    #[schemars(description = "连同还没结束的后续（后续的后续也算）一起放弃；默认 false")]
    #[serde(default)]
    pub with_after: bool,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MkdirArgs {
    #[schemars(description = "文件夹路径，如 /工作/网站改版")]
    pub path: String,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenameArgs {
    #[schemars(description = "文件夹路径；和 attachment 给且只给一个")]
    pub folder: Option<String>,
    #[schemars(with = "Option<String>", description = "附件 ID（show 列出的）；和 folder 给且只给一个")]
    pub attachment: Option<AttachmentId>,
    #[schemars(description = "新名称，不能包含 / 和 \\")]
    pub name: String,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AttachmentArgs {
    #[schemars(with = "String", description = "附件 ID（show 列出的）")]
    pub id: AttachmentId,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AttachArgs {
    #[schemars(description = "事项编号")]
    pub task: TaskRef,
    #[schemars(with = "String", description = "附件的名称，一般是文件名，如 报价单.pdf")]
    pub name: FileName,
    #[schemars(description = "文件内容的 base64，最多 20 MB")]
    pub data: String,
    #[serde(rename = "type")]
    #[schemars(with = "Option<String>", description = "文件类型，如 image/png；不给时按名称的扩展名推断")]
    pub media_type: Option<MediaType>,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DetachArgs {
    #[schemars(with = "Vec<String>", description = "要删除的附件 ID", length(min = 1))]
    pub attachments: Vec<AttachmentId>,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveArgs {
    #[serde(default)]
    #[schemars(description = "要移动的事项编号")]
    pub tasks: Vec<TaskRef>,
    #[serde(default)]
    #[schemars(description = "要移动的文件夹路径")]
    pub folders: Vec<String>,
    #[schemars(description = "目标文件夹路径，必须已存在；/ 是根目录")]
    pub to: String,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StyleArgs {
    #[schemars(description = "文件夹路径")]
    pub folder: String,
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(
        with = "Option<String>",
        description = "颜色：预设的 red、orange、amber、yellow、lime、green、teal、cyan、blue、indigo、purple、pink、brown、gray，或自己调的 #rrggbb；null 改回默认；不给就不改"
    )]
    pub color: Option<Option<FolderColor>>,
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(
        with = "Option<String>",
        description = "图标：briefcase、graduation-cap、book、pen、file、laptop、code、flask、calculator、lightbulb、target、house、cart、utensils、coffee、shirt、baby、paw、heart、gift、dumbbell、bike、heart-pulse、pill、wallet、piggy-bank、plane、car、map-pin、music、camera、palette、gamepad、star、flag、rocket、trophy、wrench、leaf、users；null 改回默认；不给就不改"
    )]
    pub icon: Option<Option<FolderIcon>>,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetPrefsArgs {
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(
        with = "Option<RemindInput>",
        description = "有具体时间的事项设截止时间时默认带的提醒，整个换掉原来的：提前多久写 \"30m\"、\"2h\"、\"1d\" 或分钟数，0 是截止时，如 [\"30m\", 0]；null 或 [] 不带；不给就不改"
    )]
    pub remind_timed: Option<Option<RemindText>>,
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(
        with = "Option<RemindInput>",
        description = "只有日期的事项设截止时间时默认带的提醒，整个换掉原来的：截止那天的几点 \"09:00\"，提前几天的几点 \"1d 20:00\"，整天的 \"1d\" 是提前那么多天的 09:00；null 或 [] 不带；不给就不改"
    )]
    pub remind_dated: Option<Option<RemindText>>,
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(
        with = "Option<String>",
        description = "主题色：预设的 emerald（默认）、lime、cyan、blue、indigo、purple、pink，或自己调的 #rrggbb；null 改回默认；不给就不改"
    )]
    pub accent: Option<Option<Accent>>,
    #[schemars(description = "日历里有没有时间轴视图，默认 true")]
    pub time_grid: Option<bool>,
    #[schemars(description = "日历里标不标国家规定的放假和补班，默认 true")]
    pub holidays: Option<bool>,
    #[schemars(with = "Option<u8>", description = "文件夹里最近完成的几项划掉后留在原处，0–20，默认 0")]
    pub keep_done: Option<KeepDone>,
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(with = "Option<String>", description = "快速添加栏固定放进的文件夹路径，/ 是根目录；null 改回用上次选的；不给就不改")]
    pub add_to: Option<Option<String>>,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CopyArgs {
    #[schemars(description = "要复制的文件夹路径")]
    pub folder: String,
    #[schemars(description = "副本放进哪个已有的文件夹，/ 是根目录；不给就在原文件夹的同一层（他人共享给我的文件夹在根目录）")]
    pub to: Option<String>,
    #[schemars(description = "副本的名字；不给时用原名，那里已有同名的就叫“原名 副本”“原名 副本 2”……")]
    pub name: Option<String>,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteArgs {
    #[serde(default)]
    #[schemars(description = "要删除的事项编号")]
    pub tasks: Vec<TaskRef>,
    #[serde(default)]
    #[schemars(description = "要删除的文件夹路径，其中的内容一并删除")]
    pub folders: Vec<String>,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DepArgs {
    #[schemars(description = "前置事项的编号")]
    pub before: TaskRef,
    #[schemars(description = "后续事项的编号")]
    pub after: TaskRef,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShareArgs {
    #[schemars(description = "自己的文件夹的路径")]
    pub folder: String,
    #[schemars(description = "共享给这些人：他们登录用的用户名", length(min = 1))]
    pub users: Vec<String>,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UnshareArgs {
    #[schemars(description = "共享的文件夹的路径")]
    pub folder: String,
    #[serde(default)]
    #[schemars(description = "要移除的成员的用户名；共享人不写就是停止共享，成员不写就是自己退出")]
    pub users: Vec<String>,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyArgs {
    pub plan: Plan,
    #[serde(default)]
    #[schemars(description = "只预览会发生的变化，不保存")]
    pub dry_run: bool,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    #[schemars(description = "文件夹路径；默认根目录 /；不存在时一起新建")]
    pub folder: Option<String>,
    #[schemars(length(min = 1))]
    pub tasks: Vec<PlanTask>,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanTask {
    #[serde(rename = "ref")]
    #[schemars(rename = "ref", description = "计划内的临时名称，供 after、before 引用；不可重复")]
    pub name: Option<String>,
    #[schemars(with = "String", description = "标题，以动作表述")]
    pub title: Title,
    #[schemars(with = "Option<String>", description = "描述，Markdown；写清怎样算完成")]
    pub description: Option<Description>,
    #[schemars(description = "文件夹：相对计划的 folder，以 / 开头则是完整路径；不存在时一起新建")]
    pub folder: Option<String>,
    #[schemars(
        with = "Option<String>",
        description = "开始时间：2026-11-14 或 2026-11-14 09:30，农历的写法同 due，没到时事项等着，不进“接下来”；不能晚于截止"
    )]
    pub start: Option<WhenText>,
    #[schemars(
        with = "Option<String>",
        description = "截止时间：2026-11-20 或 2026-11-20 18:00；按农历的写 农历八月十五、农历腊月廿三 18:00，是今天起最近的那一天（小月没有三十的取最后一天，除夕写农历腊月三十），不写月份的 农历初一 是今天起的第一个初一；配上按农历每个月几号的重复（如 1lm:1,15）时，从写的那个月起（不写月份或写的是本月的，从现在起）、不早于开始时间，取这几天里最先到的那天，要定下第一次的写公历日期"
    )]
    pub due: Option<WhenText>,
    #[schemars(
        with = "Option<String>",
        description = "代替 due：前置（after）都完成后多久以内截止，30d（天）、2w（周）或 3d 18:00，从前置都完成的那天起算，0d 是当天。用在期限从另一件事做完时算的事项上，如拿到发票后 30 天内报销；没有前置时就从今天算。remind 照样给，不能和 repeat 一起用"
    )]
    pub due_after: Option<DueAfterText>,
    #[schemars(
        with = "Option<RemindInput>",
        description = "提醒，一个或几个（列表）：提前多久写 \"30m\"、\"2h\"、\"1d\"、\"1w\" 或分钟数，0 是截止时，要有带具体时间的截止（只有日期的截止上，整天的如 \"1d\"、0 是提前那么多天的 09:00）；\"09:00\" 是截止那天的几点，\"1d 20:00\" 是提前 1 天的 20:00，只有日期的截止也能设；按开始时间算的前面加 start，如 \"start 30m\"、\"start 0\"，要有开始时间；某个时刻提醒一次的写日期和几点，如 \"2026-12-15 10:00\"，没有截止也能设。如 [\"30m\", \"1d 20:00\"]"
    )]
    pub remind: Option<RemindText>,
    #[schemars(
        with = "Option<String>",
        description = "重复：间隔数加单位，1d 每天、2w 每两周、3m 每三个月、1y 每年；按周的可以在冒号后写周几（1 周一到 7 周日），1w:1,2,3,4,5 每周一到周五、1w:1,3,5 每周一三五；workdays 每个法定工作日（周一到周五，跳过放假、算上补班的周末），用户说每个工作日的一般是它；按月的可以在冒号后写几号（1–31，没有这天的月份取最后一天），1m:1,15 每月 1 号和 15 号、1m:31 每月最后一天；按农历的在单位前加 l，可以在冒号后写几号（1–30，初一到三十），不写的是截止日期的农历日子：1ly 每年农历的这一天、1lm:1,15 农历每月初一和十五；要有截止时间。完成时顺延截止时间，不会真的完成（repeat_until 前的最后一次除外）"
    )]
    pub repeat: Option<RepeatText>,
    #[schemars(with = "Option<String>", description = "下一次从哪天算：due 按截止日期（默认），done 从完成那天；只能和 repeat 一起给")]
    pub repeat_from: Option<RepeatFrom>,
    #[schemars(
        with = "Option<String>",
        description = "重复到哪天（YYYY-MM-DD，含这天），不给就一直重复；做完最后一次，事项就完成了。只能和 repeat 一起给"
    )]
    pub repeat_until: Option<Date>,
    #[serde(default)]
    #[schemars(description = "负责人的用户名：共享的文件夹里才能设")]
    pub assign: Vec<String>,
    #[serde(default)]
    #[schemars(description = "前置：计划里的 ref，或已有事项的编号")]
    pub after: Vec<PlanRef>,
    #[serde(default)]
    #[schemars(description = "后续：计划里的 ref，或已有事项的编号")]
    pub before: Vec<PlanRef>,
}

/// A task in a plan: an existing task by number (`42`, `"#42"` or `"@alice#42"`), or a planned one by
/// `ref`.
#[derive(Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PlanRef {
    Number(i64),
    Name(String),
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UndoArgs {
    #[schemars(description = "批次号")]
    pub batch: i64,
}

/// A date as typed, with a time or without: `2026-11-20` or `2026-11-20 18:00`; or the next day so in
/// the lunar calendar, `农历八月十五`, `农历初一 09:00`.
#[derive(Clone, Deserialize)]
#[serde(try_from = "String")]
pub struct WhenText {
    day: DayText,
    pub time: Option<Time>,
}

/// A repeat on some days of every lunar month, and the start of the task repeating so: its first time
/// is the first of those days on both.
#[derive(Clone, Copy)]
pub struct Monthly<'a> {
    pub days: Monthdays,
    pub start: Option<&'a Date>,
}

#[derive(Clone)]
enum DayText {
    Date(Date),
    /// Day 1–30 of lunar month 1–12, a leap one or not, or of any month.
    Lunar(Option<(u32, bool)>, u32),
}

impl WhenText {
    /// The day of a lunar month it says, if it says one.
    pub fn lunar_day(&self) -> Option<u32> {
        match self.day {
            DayText::Lunar(_, day) => Some(day),
            DayText::Date(_) => None,
        }
    }

    /// The date it stands for `now`: a lunar day's next, today's only if its time hasn't passed; of a task
    /// repeating `monthly`, the first of its days from the month said on, and from its start.
    pub fn date(&self, now: &Now, monthly: Option<Monthly>) -> Result<Date, String> {
        let past = || "农历日期只能到 2100 年".to_owned();
        let (month, day) = match &self.day {
            DayText::Date(d) => return Ok(d.clone()),
            DayText::Lunar(month, day) => (*month, *day),
        };
        let passed = self.time.as_ref().zip(now.time.as_ref()).is_some_and(|(at, now)| at < now);
        let from = if passed { now.today.plus(1) } else { now.today.clone() };
        let Some(Monthly { days, start }) = monthly else { return lunar::next(month, day, &from).ok_or_else(past) };
        // Of a month to come, from its first day; of the one it is, from now.
        let month_from = match month {
            Some(said) if now.today.lunar().is_some_and(|l| (l.month, l.leap) != said) => {
                Some(lunar::next(month, 1, &now.today).ok_or_else(past)?)
            }
            _ => None,
        };
        let from = [Some(from), month_from, start.cloned()].into_iter().flatten().max().expect("now is one");
        (1..=30).filter(|&d| days.has(d)).filter_map(|d| lunar::next(None, d, &from)).min().ok_or_else(past)
    }
}

impl TryFrom<String> for WhenText {
    type Error = String;

    fn try_from(s: String) -> Result<Self, String> {
        let s = s.trim();
        let (day, time) = match s.rsplit_once(char::is_whitespace) {
            Some((day, time)) if time.contains(':') => (day.trim_end(), Some(time)),
            _ => (s, None),
        };
        let day = match day.strip_prefix("农历") {
            Some(lunar) => lunar_day(lunar.trim()).ok_or_else(|| {
                format!("农历日期“{day}”格式错误，应如 农历八月十五、农历腊月廿三、农历闰六月初一，或不写月份的 农历初一（下一个初一）")
            })?,
            None => DayText::Date(day.to_owned().try_into()?),
        };
        Ok(WhenText { day, time: time.map(|t| t.to_owned().try_into()).transpose()? })
    }
}

/// A lunar day as typed after 农历: `八月十五`, `腊月廿三`, `闰六月初一`, `8月15`, or of any month, `初一`.
fn lunar_day(s: &str) -> Option<DayText> {
    let (month, day) = match s.split_once('月') {
        Some((m, day)) => {
            let (m, leap) = m.strip_prefix('闰').map_or((m, false), |m| (m, true));
            let n = match m {
                "正" => 1,
                "冬" => 11,
                "腊" => 12,
                m => number(m).filter(|n| (1..=12).contains(n))?,
            };
            (Some((n, leap)), day)
        }
        None => (None, s),
    };
    let day = day.strip_suffix('日').unwrap_or(day);
    let day = match day.strip_prefix('初') {
        Some(d) => number(d).filter(|d| *d <= 10)?,
        None => number(day)?,
    };
    (1..=30).contains(&day).then_some(DayText::Lunar(month, day))
}

/// A number up to 99 as typed in digits or in Chinese, as lunar days are: `15`, `五`, `十五`, `二十三`,
/// `廿三`, `三十`.
fn number(s: &str) -> Option<u32> {
    if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
        return s.parse().ok();
    }
    let digit = |c: &char| Some("一二三四五六七八九".chars().position(|d| d == *c)? as u32 + 1);
    let tens = |c: &char| digit(c).filter(|t| (2..=3).contains(t));
    match s.chars().collect::<Vec<_>>().as_slice() {
        ['十'] => Some(10),
        ['卅'] => Some(30),
        [d] => digit(d),
        ['十', d] => Some(10 + digit(d)?),
        ['廿', d] => Some(20 + digit(d)?),
        [t, '十'] => Some(tens(t)? * 10),
        [t, '十', d] => Some(tens(t)? * 10 + digit(d)?),
        _ => None,
    }
}

/// A due date some days after the prerequisites are done, as typed: `30d`, by the week `2w`, or with a
/// time, `3d 18:00`.
#[derive(Clone, Deserialize)]
#[serde(try_from = "String")]
pub struct DueAfterText {
    pub days: u16,
    pub time: Option<Time>,
}

impl TryFrom<String> for DueAfterText {
    type Error = String;

    fn try_from(s: String) -> Result<Self, String> {
        let s = s.trim();
        let (days, time) = s.split_once(' ').map_or((s, None), |(d, t)| (d, Some(t.trim())));
        let count = |unit: char| days.strip_suffix(unit).and_then(|n| n.parse::<u16>().ok());
        let days = count('d').or_else(|| count('w').and_then(|w| w.checked_mul(7))).ok_or("前置完成后多久以内写成 30d、2w 或 3d 18:00")?;
        Ok(DueAfterText { days, time: time.map(|t| t.to_owned().try_into()).transpose()? })
    }
}

/// `a` as it is typed: the other way round from `DueAfterText`.
pub fn due_after_text(a: &DueAfter) -> String {
    match &a.at {
        Some(at) => format!("{}d {}", a.days, at.time.as_str()),
        None => format!("{}d", a.days),
    }
}

/// Reminders as typed (docs/agent.md#引用): one or a list of minutes before the due time, `60`, or as
/// long in minutes, hours, days or weeks, `"30m"`, `"2h"`, `"1d"`, `"1w"`, `"2h30m"`; a time on the due
/// date, `"09:00"`, or so many days or weeks before it, `"1d 20:00"`, `"1w 09:00"`. By the start the
/// same after `start`, `"start 30m"`; at a time of its own, a date and the time, `"2026-12-15 10:00"`.
/// A list may also be one string, `"1d, 60"`: models write them all so.
#[derive(Clone, Default, Deserialize)]
#[serde(try_from = "RemindInput")]
pub struct RemindText {
    pub due: Vec<Remind>,
    pub start: Vec<Remind>,
    /// Each with its time.
    pub at: Vec<WhenText>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum RemindInput {
    One(RemindItem),
    Many(Vec<RemindItem>),
}

#[derive(Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum RemindItem {
    Minutes(u32),
    Text(String),
}

impl TryFrom<RemindInput> for RemindText {
    type Error = String;

    fn try_from(input: RemindInput) -> Result<Self, String> {
        let items = match input {
            RemindInput::One(item) => vec![item],
            RemindInput::Many(items) => items,
        };
        let mut all = RemindText::default();
        for item in items {
            match item {
                RemindItem::Minutes(before) => all.due.push(Remind::Before(RemindBefore { before })),
                RemindItem::Text(s) => {
                    for one in s.split([',', '，']).map(str::trim) {
                        if one.is_empty() && s.contains([',', '，']) {
                            return Err(format!("提醒“{s}”里逗号之间不能留空"));
                        }
                        let wrong = || {
                            format!(
                                "提醒“{one}”格式错误，应为提前的分钟数如 60，提前多久如 \"30m\"、\"2h\"、\"1d\"、\"1w\"，截止那天的几点如 \"09:00\"，提前几天的几点如 \"1d 20:00\"；按开始时间的前面加 start，如 \"start 30m\"；某个时刻的写日期和几点，如 \"2026-12-15 10:00\""
                            )
                        };
                        if let Some(start) = one.strip_prefix("start") {
                            let start = start.trim();
                            all.start.push(if start.is_empty() {
                                Remind::Before(RemindBefore { before: 0 })
                            } else {
                                remind(start).ok_or_else(wrong)?
                            });
                        } else if let Some(r) = remind(one) {
                            all.due.push(r);
                        } else {
                            let at = WhenText::try_from(one.to_owned()).map_err(|_| wrong())?;
                            if at.time.is_none() {
                                return Err(format!("提醒“{one}”要写几点，如 \"{one} 09:00\""));
                            }
                            all.at.push(at);
                        }
                    }
                }
            }
        }
        Ok(all)
    }
}

/// `r` as it is typed: the other way round from `remind`.
pub fn remind_text(r: &Remind) -> serde_json::Value {
    match r {
        Remind::Before(RemindBefore { before }) => (*before).into(),
        Remind::On(RemindOn { days: 0, time }) => time.as_str().into(),
        Remind::On(RemindOn { days, time }) => format!("{days}d {}", time.as_str()).into(),
    }
}

/// One by the start as it is typed: `start 60`, `start 09:00`, `start 1d 20:00`.
pub fn start_remind_text(r: &Remind) -> String {
    match r {
        Remind::Before(RemindBefore { before }) => format!("start {before}"),
        Remind::On(RemindOn { days: 0, time }) => format!("start {}", time.as_str()),
        Remind::On(RemindOn { days, time }) => format!("start {days}d {}", time.as_str()),
    }
}

/// One reminder as typed, if it is one.
fn remind(s: &str) -> Option<Remind> {
    // A time on the due date, or so many days or weeks before it: `09:00`, `1d 20:00`, `1w 09:00`.
    if s.contains(':') {
        let (before, time) = s.rsplit_once(char::is_whitespace).unwrap_or(("0d", s));
        let before = before.trim_end();
        let days = match before.strip_suffix('w') {
            Some(weeks) => weeks.parse::<u32>().ok()?.checked_mul(7)?,
            None => before.strip_suffix('d')?.parse().ok()?,
        };
        return Some(Remind::On(RemindOn { days, time: Time::try_from(time.to_owned()).ok()? }));
    }
    // So many minutes before the due time: `60`, or as long in minutes, hours, days or weeks, `2h30m`.
    s.parse().ok().or_else(|| minutes(s)).map(|before| Remind::Before(RemindBefore { before }))
}

/// The minutes in `s`, a length of time in minutes, hours, days and weeks: `30m`, `2h30m`, `1d 2h`.
fn minutes(s: &str) -> Option<u32> {
    let (mut total, mut number) = (0u32, None::<u32>);
    for c in s.chars() {
        let unit = match c {
            // Between a unit and the next number only: `1 2m` is no 12 minutes.
            _ if c.is_whitespace() && number.is_none() => continue,
            'm' => 1,
            'h' => 60,
            'd' => 1440,
            'w' => 7 * 1440,
            _ => {
                number = Some(number.unwrap_or(0).checked_mul(10)?.checked_add(c.to_digit(10)?)?);
                continue;
            }
        };
        total = total.checked_add(number.take()?.checked_mul(unit)?)?;
    }
    // Each number with its unit, and at least one.
    (number.is_none() && !s.trim().is_empty()).then_some(total)
}

/// How every statutory working day is typed (docs/design/repeat.md#重复).
pub const LEGAL_WORKDAYS: &str = "workdays";

/// A repeat as typed: `1d`, `2w`, `3m`, `1y` (12 months); weekly on some days, Monday 1 to Sunday 7,
/// `1w:1,3,5`; monthly on some days of the month, `1m:1,15`; by the lunar calendar on some days, 1–30,
/// `1lm:1,15`, `1ly:15`, or on the due date's, `1ly`; every statutory working day, `workdays`.
#[derive(Clone, Copy, Deserialize)]
#[serde(try_from = "String")]
pub struct RepeatText {
    pub every: Every,
    pub period: PeriodText,
}

/// What a repeat goes by: as typed, or by the lunar calendar on the due date's day in it, which is
/// known only once the due date is.
#[derive(Clone, Copy)]
pub enum PeriodText {
    Is(Period),
    LunarDueDay,
}

impl TryFrom<String> for RepeatText {
    type Error = String;

    fn try_from(s: String) -> Result<Self, String> {
        let wrong = || {
            format!(
                "重复“{s}”格式错误，应为间隔数加单位，如 1d、2w、3m、1y；按周的可以在冒号后写周几，如 1w:1,3,5；按月的可以写几号，如 1m:1,15；按农历的在单位前加 l，可以写几号，如 1ly、1lm:1,15；每个法定工作日写 workdays"
            )
        };
        if s.trim() == LEGAL_WORKDAYS {
            return Ok(RepeatText { every: 1.try_into()?, period: PeriodText::Is(Period::LegalWorkdays) });
        }
        let (interval, days) = s.trim().split_once(':').map_or((s.trim(), None), |(i, d)| (i, Some(d)));
        let Some(unit) = interval.chars().last() else { return Err(wrong()) };
        let n = &interval[..interval.len() - unit.len_utf8()];
        let (n, lunar) = n.strip_suffix('l').map_or((n, false), |n| (n, true));
        let n: u32 = n.parse().map_err(|_| wrong())?;
        let days =
            days.map(|d| d.split(',').map(|d| d.trim().parse::<u8>().map_err(|_| wrong())).collect::<Result<Vec<_>, _>>()).transpose()?;
        let lunar_on = |days: Option<Vec<u8>>| -> Result<PeriodText, String> {
            Ok(match days {
                Some(days) => PeriodText::Is(Period::LunarMonth(Monthdays::lunar(days)?)),
                None => PeriodText::LunarDueDay,
            })
        };
        let (n, period) = match (unit, days, lunar) {
            ('d', None, false) => (n, PeriodText::Is(Period::Day)),
            ('w', days, false) => (n, PeriodText::Is(Period::Week(days.map(Weekdays::try_from).transpose()?))),
            ('m', days, false) => (n, PeriodText::Is(Period::Month(days.map(Monthdays::try_from).transpose()?))),
            ('y', None, false) => (n.saturating_mul(12), PeriodText::Is(Period::Month(None))),
            ('m', days, true) => (n, lunar_on(days)?),
            ('y', days, true) => (n.saturating_mul(12), lunar_on(days)?),
            _ => return Err(wrong()),
        };
        Ok(RepeatText { every: n.try_into()?, period })
    }
}

/// Tells a missing field (`None`) from an explicit `null` (`Some(None)`).
fn nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<Option<T>>, D::Error> {
    Option::deserialize(d).map(Some)
}
