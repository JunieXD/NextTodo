//! Tool arguments. Descriptions are written for the agents that read the tool list.

use nexttodo_core::model::{Date, Description, Every, Monthdays, Name, Period, RepeatFrom, Time, Title, Weekdays};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoArgs {}

#[derive(Deserialize, JsonSchema)]
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

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NextArgs {
    #[schemars(description = "最多列出多少项，默认 50，最多 500", range(min = 1, max = 500))]
    pub limit: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShowArgs {
    #[schemars(description = "事项编号")]
    pub task: i64,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchArgs {
    #[schemars(description = "关键词")]
    pub query: String,
    #[schemars(description = "最多列出多少项，默认 50，最多 500", range(min = 1, max = 500))]
    pub limit: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
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

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LogArgs {
    #[schemars(description = "条数，默认 10，最多 100", range(min = 1, max = 100))]
    pub limit: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddArgs {
    #[schemars(with = "String", description = "标题，以动作表述")]
    pub title: Title,
    #[schemars(description = "文件夹路径；默认根目录 /；不存在时一起新建")]
    pub folder: Option<String>,
    #[schemars(with = "Option<String>", description = "描述，Markdown；写清怎样算完成")]
    pub description: Option<Description>,
    #[schemars(with = "Option<String>", description = "截止时间：2026-11-20 或 2026-11-20 18:00")]
    pub due: Option<DueText>,
    #[schemars(description = "提前多少分钟提醒，0 表示截止时；截止时间要带具体时间")]
    pub remind: Option<u32>,
    #[schemars(
        with = "Option<String>",
        description = "重复：间隔数加单位，1d 每天、2w 每两周、3m 每三个月、1y 每年；按周的可以在冒号后写周几（1 周一到 7 周日），1w:1,2,3,4,5 每个工作日、1w:1,3,5 每周一三五；按月的可以在冒号后写几号（1–31，没有这天的月份取最后一天），1m:1,15 每月 1 号和 15 号、1m:31 每月最后一天；要有截止时间。完成时顺延截止时间，不会真的完成"
    )]
    pub repeat: Option<RepeatText>,
    #[schemars(with = "Option<String>", description = "下一次从哪天算：due 按截止日期（默认），done 从完成那天；只能和 repeat 一起给")]
    pub repeat_from: Option<RepeatFrom>,
    #[serde(default)]
    #[schemars(description = "前置事项的编号：它们完成后才能做这一项")]
    pub after: Vec<i64>,
    #[serde(default)]
    #[schemars(description = "后续事项的编号：这一项完成后才能做它们")]
    pub before: Vec<i64>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditArgs {
    #[schemars(description = "事项编号")]
    pub task: i64,
    #[schemars(with = "Option<String>", description = "新标题")]
    pub title: Option<Title>,
    #[schemars(with = "Option<String>", description = "新描述，Markdown，整段替换；只改其中一段时用 replace 或 append")]
    pub description: Option<Description>,
    #[serde(default)]
    #[schemars(
        description = "在服务端当时的描述上依次替换：old 必须恰好出现一次，找不到说明已被改过，先 show 再改。别处同时改的内容不受影响"
    )]
    pub replace: Vec<Replace>,
    #[schemars(description = "在描述末尾另起一行追加，在 replace 之后")]
    pub append: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(
        with = "Option<String>",
        description = "截止时间：2026-11-20 或 2026-11-20 18:00；null 清除。不给 remind 时，新时间带具体时间就保留原来的提醒"
    )]
    pub due: Option<Option<DueText>>,
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(description = "提前多少分钟提醒，0 表示截止时；null 清除")]
    pub remind: Option<Option<u32>>,
    #[serde(default, deserialize_with = "nullable")]
    #[schemars(
        with = "Option<String>",
        description = "重复：间隔数加单位，1d 每天、2w 每两周、3m 每三个月、1y 每年；按周的可以在冒号后写周几（1 周一到 7 周日），1w:1,2,3,4,5 每个工作日、1w:1,3,5 每周一三五；按月的可以在冒号后写几号（1–31，没有这天的月份取最后一天），1m:1,15 每月 1 号和 15 号、1m:31 每月最后一天；null 不再重复。只改截止时间时重复保留"
    )]
    pub repeat: Option<Option<RepeatText>>,
    #[schemars(with = "Option<String>", description = "下一次从哪天算：due 按截止日期（默认），done 从完成那天；只能和 repeat 一起给")]
    pub repeat_from: Option<RepeatFrom>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Replace {
    #[schemars(description = "描述里现有的一段原文")]
    pub old: String,
    #[schemars(description = "换成的内容，可以为空")]
    pub new: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TasksArgs {
    #[schemars(description = "事项编号", length(min = 1))]
    pub tasks: Vec<i64>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DoneArgs {
    #[schemars(description = "事项编号", length(min = 1))]
    pub tasks: Vec<i64>,
    #[schemars(with = "Option<String>", description = "用户当地今天的日期，如 2026-11-08；有重复的事项时必须给，下一次的截止时间据此计算")]
    pub today: Option<Date>,
    #[schemars(description = "重复的事项结束重复、直接完成，如读完一本书时；默认 false")]
    #[serde(default)]
    pub end_repeat: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MkdirArgs {
    #[schemars(description = "文件夹路径，如 /工作/网站改版")]
    pub path: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenameArgs {
    #[schemars(description = "文件夹路径")]
    pub folder: String,
    #[schemars(with = "String", description = "新名称，不能包含 /")]
    pub name: Name,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveArgs {
    #[serde(default)]
    #[schemars(description = "要移动的事项编号")]
    pub tasks: Vec<i64>,
    #[serde(default)]
    #[schemars(description = "要移动的文件夹路径")]
    pub folders: Vec<String>,
    #[schemars(description = "目标文件夹路径，必须已存在；/ 是根目录")]
    pub to: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteArgs {
    #[serde(default)]
    #[schemars(description = "要删除的事项编号")]
    pub tasks: Vec<i64>,
    #[serde(default)]
    #[schemars(description = "要删除的文件夹路径，其中的内容一并删除")]
    pub folders: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DepArgs {
    #[schemars(description = "前置事项的编号")]
    pub before: i64,
    #[schemars(description = "后续事项的编号")]
    pub after: i64,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyArgs {
    pub plan: Plan,
    #[serde(default)]
    #[schemars(description = "只预览会发生的变化，不保存")]
    pub dry_run: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    #[schemars(description = "文件夹路径；默认根目录 /；不存在时一起新建")]
    pub folder: Option<String>,
    #[schemars(length(min = 1))]
    pub tasks: Vec<PlanTask>,
}

#[derive(Deserialize, JsonSchema)]
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
    #[schemars(with = "Option<String>", description = "截止时间：2026-11-20 或 2026-11-20 18:00")]
    pub due: Option<DueText>,
    #[schemars(description = "提前多少分钟提醒，0 表示截止时；截止时间要带具体时间")]
    pub remind: Option<u32>,
    #[schemars(
        with = "Option<String>",
        description = "重复：间隔数加单位，1d 每天、2w 每两周、3m 每三个月、1y 每年；按周的可以在冒号后写周几（1 周一到 7 周日），1w:1,2,3,4,5 每个工作日、1w:1,3,5 每周一三五；按月的可以在冒号后写几号（1–31，没有这天的月份取最后一天），1m:1,15 每月 1 号和 15 号、1m:31 每月最后一天；要有截止时间。完成时顺延截止时间，不会真的完成"
    )]
    pub repeat: Option<RepeatText>,
    #[schemars(with = "Option<String>", description = "下一次从哪天算：due 按截止日期（默认），done 从完成那天；只能和 repeat 一起给")]
    pub repeat_from: Option<RepeatFrom>,
    #[serde(default)]
    #[schemars(description = "前置：计划里的 ref，或已有事项的编号")]
    pub after: Vec<PlanRef>,
    #[serde(default)]
    #[schemars(description = "后续：计划里的 ref，或已有事项的编号")]
    pub before: Vec<PlanRef>,
}

/// A task in a plan: an existing task by number (`42` or `"#42"`), or a planned one by `ref`.
#[derive(Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PlanRef {
    Number(i64),
    Name(String),
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UndoArgs {
    #[schemars(description = "批次号")]
    pub batch: i64,
}

/// A due date as typed: `2026-11-20` or `2026-11-20 18:00`.
#[derive(Clone, Deserialize)]
#[serde(try_from = "String")]
pub struct DueText {
    pub date: Date,
    pub time: Option<Time>,
}

impl TryFrom<String> for DueText {
    type Error = String;

    fn try_from(s: String) -> Result<Self, String> {
        let s = s.trim();
        let (date, time) = s.split_once(' ').map_or((s, None), |(d, t)| (d, Some(t.trim())));
        Ok(DueText { date: date.to_owned().try_into()?, time: time.map(|t| t.to_owned().try_into()).transpose()? })
    }
}

/// A repeat as typed: `1d`, `2w`, `3m`, `1y` (12 months); weekly on some days, Monday 1 to Sunday 7,
/// `1w:1,3,5`; monthly on some days of the month, `1m:1,15`.
#[derive(Clone, Copy, Deserialize)]
#[serde(try_from = "String")]
pub struct RepeatText {
    pub every: Every,
    pub period: Period,
}

impl TryFrom<String> for RepeatText {
    type Error = String;

    fn try_from(s: String) -> Result<Self, String> {
        let wrong = || {
            format!(
                "重复“{s}”格式错误，应为间隔数加单位，如 1d、2w、3m、1y；按周的可以在冒号后写周几，如 1w:1,3,5；按月的可以写几号，如 1m:1,15"
            )
        };
        let (interval, days) = s.trim().split_once(':').map_or((s.trim(), None), |(i, d)| (i, Some(d)));
        let Some(unit) = interval.chars().last() else { return Err(wrong()) };
        let n: u32 = interval[..interval.len() - unit.len_utf8()].parse().map_err(|_| wrong())?;
        let days =
            days.map(|d| d.split(',').map(|d| d.trim().parse::<u8>().map_err(|_| wrong())).collect::<Result<Vec<_>, _>>()).transpose()?;
        let (n, period) = match (unit, days) {
            ('d', None) => (n, Period::Day),
            ('w', days) => (n, Period::Week(days.map(Weekdays::try_from).transpose()?)),
            ('m', days) => (n, Period::Month(days.map(Monthdays::try_from).transpose()?)),
            ('y', None) => (n.saturating_mul(12), Period::Month(None)),
            _ => return Err(wrong()),
        };
        Ok(RepeatText { every: n.try_into()?, period })
    }
}

/// Tells a missing field (`None`) from an explicit `null` (`Some(None)`).
fn nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<Option<T>>, D::Error> {
    Option::deserialize(d).map(Some)
}
