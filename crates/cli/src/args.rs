//! The commands and their arguments, as `nexttodo --help` shows them.

use crate::VERSION;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "nexttodo", version = VERSION, about = "NextTodo 命令行：查看和管理待办")]
pub struct Cli {
    #[arg(long, global = true, help = "输出 JSON 数据")]
    pub json: bool,
    #[arg(
        long,
        global = true,
        value_name = "UUIDv7",
        help = "写操作的改动 ID：报“可能已经生效”时，用错误里给的 ID 重发同一条命令，生效过的不会再生效一次"
    )]
    pub request_id: Option<String>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    #[command(about = "用用户名和密码登录，生成令牌存到配置文件")]
    Login {
        #[arg(long, help = "服务器地址，如 https://todo.example.com")]
        url: Option<String>,
        #[arg(long, default_value = "命令行", help = "令牌名，会显示在改动记录里")]
        name: String,
        #[arg(long, help = "生成只读令牌")]
        read_only: bool,
    },
    #[command(about = "文件夹树")]
    Tree,
    #[command(about = "列出文件夹里的子文件夹和事项")]
    Ls {
        #[arg(help = "文件夹路径，默认根目录")]
        folder: Option<String>,
        #[arg(long, help = "包括已完成的")]
        all: bool,
        #[arg(long, help = "最多列出多少项，默认 50，最多 500")]
        limit: Option<u32>,
    },
    #[command(about = "接下来：现在就能做的事项")]
    Next {
        #[arg(long = "folder", num_args = 1.., value_name = "路径", help = "只列出这些文件夹（含子文件夹）里的事项")]
        folders: Option<Vec<String>>,
        #[arg(long, help = "最多列出多少项，默认 50，最多 500")]
        limit: Option<u32>,
    },
    #[command(about = "日程：今天起几天每天截止的事项，和已逾期的")]
    Agenda {
        #[arg(long, help = "列今天起几天，默认 7，最多 90")]
        days: Option<u32>,
        #[arg(long = "folder", num_args = 1.., value_name = "路径", help = "只列出这些文件夹（含子文件夹）里的事项")]
        folders: Option<Vec<String>>,
    },
    #[command(about = "事项详情")]
    Show {
        #[arg(value_parser = task, help = TASK)]
        task: String,
    },
    #[command(about = "搜索事项的标题和描述")]
    Search {
        #[arg(required = true, help = "关键词")]
        query: Vec<String>,
        #[arg(long, help = "最多列出多少项，默认 50，最多 500")]
        limit: Option<u32>,
    },
    #[command(about = "文件夹里的事项和依赖")]
    Graph {
        #[arg(help = "文件夹路径，默认全部")]
        folder: Option<String>,
        #[arg(long, help = "包括已完成的")]
        all: bool,
        #[arg(long, help = "输出 Mermaid 流程图")]
        mermaid: bool,
    },
    #[command(about = "最近的改动批次，或一个批次的全部改动")]
    Log {
        #[arg(help = "只看这一个批次，每类变化列出全部事项")]
        batch: Option<i64>,
        #[arg(short = 'n', default_value_t = 10, help = "条数，最多 100")]
        limit: u32,
        #[arg(long, help = "只列这个批次之前的，用来往前翻")]
        before: Option<i64>,
        #[arg(long, help = "只列删除了事项、文件夹或附件的批次")]
        deleted: bool,
    },
    #[command(about = "AI 整理的请求：等待、正在整理和失败的")]
    Jobs,
    #[command(about = "借网盘同步时立即同步")]
    Sync,
    #[command(about = "添加一个事项")]
    Add {
        title: String,
        #[arg(long, help = "文件夹路径，不存在时一起新建")]
        folder: Option<String>,
        #[arg(short, long, allow_hyphen_values = true, help = "描述，Markdown")]
        description: Option<String>,
        #[arg(
            long,
            help = "开始时间：2026-11-14 或 \"2026-11-14 09:30\"，按农历的写 农历八月十五、\"农历腊月廿三 18:00\"（今天起最近的那天），没到时事项等着"
        )]
        start: Option<String>,
        #[arg(
            long,
            conflicts_with = "due_after",
            help = "截止时间：2026-11-20 或 \"2026-11-20 18:00\"，按农历的写 农历八月十五、\"农历腊月廿三 18:00\"（今天起最近的那天）；配 1lm:1,15 这类重复时，从写的那个月起、不早于开始时间，取这几天里最先到的那天，要定下第一次的写公历日期"
        )]
        due: Option<String>,
        #[arg(
            long,
            value_name = "天数",
            help = "代替 --due：前置（--after）都完成后多久以内截止，30d、2w 或 \"3d 18:00\"，从前置都完成的那天起算，0d 是当天；没有前置时从今天算"
        )]
        due_after: Option<String>,
        #[arg(long, num_args = 1.., value_name = "提醒", help = "提醒，可以几个：提前多久 30m、2h、1d、1w 或分钟数（0 是截止时），或截止那天的几点 09:00、提前几天的几点 \"1d 20:00\"；按开始时间算的前面加 start，如 \"start 30m\"；某个时刻提醒一次的写 \"2026-12-15 10:00\"")]
        remind: Vec<String>,
        #[arg(
            long,
            value_name = "间隔",
            help = "重复：1d 每天、2w 每两周、3m 每三个月、1y 每年，1w:1,2,3,4,5 每周一到周五、1w:1,3,5 每周一三五，workdays 每个法定工作日（周一到周五，跳过放假、算上补班的周末），1m:1,15 每月 1 号和 15 号、1m:31 每月最后一天；按农历的在单位前加 l，可以在冒号后写几号（1–30，初一到三十），不写的是截止日期的农历日子：1ly 每年农历的这一天、1lm:1,15 农历每月初一和十五；要有截止时间"
        )]
        repeat: Option<String>,
        #[arg(long, requires = "repeat", value_parser = ["due", "done"], help = "下一次从哪天算：due 按截止日期（默认），done 从完成那天")]
        repeat_from: Option<String>,
        #[arg(
            long,
            requires = "repeat",
            value_name = "日期",
            help = "重复到哪天（YYYY-MM-DD，含这天），不给就一直重复；做完最后一次，事项就完成了"
        )]
        repeat_until: Option<String>,
        #[arg(long, num_args = 1.., value_name = "用户名", help = "负责人：共享的文件夹里的共享人或成员")]
        assign: Vec<String>,
        #[arg(long, num_args = 1.., value_parser = task, help = "前置事项的编号")]
        after: Vec<String>,
        #[arg(long, num_args = 1.., value_parser = task, help = "后续事项的编号")]
        before: Vec<String>,
    },
    #[command(about = "修改事项")]
    Edit {
        #[arg(value_parser = task, help = TASK)]
        task: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(short, long, allow_hyphen_values = true, help = "描述，整段替换")]
        description: Option<String>,
        #[arg(
            long,
            num_args = 2,
            allow_hyphen_values = true,
            value_names = ["原文", "新文"],
            conflicts_with = "description",
            help = "把描述里恰好出现一次的原文换成新文，可以重复"
        )]
        replace: Vec<String>,
        #[arg(long, allow_hyphen_values = true, conflicts_with = "description", help = "在描述末尾另起一行追加")]
        append: Option<String>,
        #[arg(
            long,
            conflicts_with = "no_start",
            help = "开始时间：2026-11-14 或 \"2026-11-14 09:30\"，按农历的写 农历八月十五、\"农历腊月廿三 18:00\"（今天起最近的那天）；只写日期的去掉原来的几点"
        )]
        start: Option<String>,
        #[arg(long, help = "清除开始时间")]
        no_start: bool,
        #[arg(long, conflicts_with_all = ["no_due", "due_after"], help = "截止时间：2026-11-20 或 \"2026-11-20 18:00\"，按农历的写 农历八月十五、\"农历腊月廿三 18:00\"（今天起最近的那天）；配 1lm:1,15 这类重复时，从写的那个月起、不早于开始时间，取这几天里最先到的那天，要定下第一次的写公历日期")]
        due: Option<String>,
        #[arg(
            long,
            value_name = "天数",
            conflicts_with = "no_due",
            help = "代替 --due：前置都完成后多久以内截止，30d、2w 或 \"3d 18:00\"，从前置都完成的那天起算，0d 是当天；前置已经都完成或没有前置的，从今天算"
        )]
        due_after: Option<String>,
        #[arg(long, help = "清除截止时间，前置完成后几天内的也一样")]
        no_due: bool,
        #[arg(long, num_args = 1.., value_name = "提醒", conflicts_with = "no_remind", help = "提醒，可以几个：提前多久 30m、2h、1d、1w 或分钟数（0 是截止时），或截止那天的几点 09:00、提前几天的几点 \"1d 20:00\"；按开始时间算的前面加 start，如 \"start 30m\"；某个时刻提醒一次的写 \"2026-12-15 10:00\"，整个换掉原来的")]
        remind: Vec<String>,
        #[arg(long, help = "清除提醒")]
        no_remind: bool,
        #[arg(
            long,
            value_name = "间隔",
            conflicts_with = "no_repeat",
            help = "重复：1d 每天、2w 每两周、3m 每三个月、1y 每年，1w:1,2,3,4,5 每周一到周五、1w:1,3,5 每周一三五，workdays 每个法定工作日（周一到周五，跳过放假、算上补班的周末），1m:1,15 每月 1 号和 15 号、1m:31 每月最后一天；按农历的在单位前加 l，不写几号的是截止日期的农历日子，1ly 每年农历的这一天、1lm:1,15 农历每月初一和十五"
        )]
        repeat: Option<String>,
        #[arg(long, requires = "repeat", value_parser = ["due", "done"], help = "下一次从哪天算：due 按截止日期（默认），done 从完成那天")]
        repeat_from: Option<String>,
        #[arg(
            long,
            requires = "repeat",
            value_name = "日期",
            help = "重复到哪天（YYYY-MM-DD，含这天），不给就一直重复；做完最后一次，事项就完成了"
        )]
        repeat_until: Option<String>,
        #[arg(long, help = "不再重复")]
        no_repeat: bool,
        #[arg(long, num_args = 1.., value_name = "用户名", help = "加上负责人")]
        assign: Vec<String>,
        #[arg(long, num_args = 1.., value_name = "用户名", help = "去掉负责人")]
        unassign: Vec<String>,
        #[arg(long, num_args = 1..=2, value_name = "时间", help = "修改完成时间：YYYY-MM-DD 或 YYYY-MM-DD HH:MM")]
        done_at: Vec<String>,
    },
    #[command(about = "标为完成")]
    Done {
        #[arg(required = true, value_parser = task, help = TASK)]
        tasks: Vec<String>,
        #[arg(long, help = "重复的事项结束重复、直接完成，如读完一本书时")]
        end_repeat: bool,
        #[arg(long, num_args = 1..=2, value_name = "时间", help = "补记完成：YYYY-MM-DD 或 YYYY-MM-DD HH:MM")]
        at: Vec<String>,
    },
    #[command(about = "放弃：不做了，和完成一样不再挡着后续；重复的事项整个结束")]
    Drop {
        #[arg(required = true, value_parser = task, help = TASK)]
        tasks: Vec<String>,
        #[arg(long, help = "连同还没结束的后续（后续的后续也算）一起放弃")]
        with_after: bool,
    },
    #[command(about = "重新打开已完成或放弃的事项")]
    Reopen {
        #[arg(required = true, value_parser = task, help = TASK)]
        tasks: Vec<String>,
    },
    #[command(about = "跳过重复事项的这一次：顺延到下一次，不算完成")]
    Skip {
        #[arg(required = true, value_parser = task, help = TASK)]
        tasks: Vec<String>,
    },
    #[command(about = "新建文件夹，缺的上级一起建")]
    Mkdir { path: String },
    #[command(about = "文件夹或附件改名")]
    Rename {
        #[arg(value_name = "文件夹或附件 ID", help = "文件夹路径；带 --attachment 时是附件 ID")]
        target: String,
        name: String,
        #[arg(long, help = "给附件改名")]
        attachment: bool,
    },
    #[command(about = "移动事项（编号）或文件夹（路径）到另一个文件夹")]
    Mv {
        #[arg(required = true, num_args = 2.., help = "要移动的事项和文件夹，最后一个是目标文件夹")]
        items: Vec<String>,
    },
    #[command(about = "设文件夹的颜色和图标")]
    Style {
        folder: String,
        #[arg(
            long,
            conflicts_with = "no_color",
            help = "预设的 red、orange、amber、yellow、lime、green、teal、cyan、blue、indigo、purple、pink、brown、gray，或自己调的 #rrggbb"
        )]
        color: Option<String>,
        #[arg(long, help = "改回默认的颜色")]
        no_color: bool,
        #[arg(long, conflicts_with = "no_icon", help = "图标的名字，见 docs/data-model.md 的 folders，如 briefcase、book、house")]
        icon: Option<String>,
        #[arg(long, help = "改回默认的图标")]
        no_icon: bool,
    },
    #[command(about = "复制文件夹，连同里面的事项、附件和依赖，事项都变回待办")]
    Cp {
        folder: String,
        #[arg(value_name = "目标文件夹", help = "副本放进的文件夹，默认是原文件夹的同一层")]
        to: Option<String>,
        #[arg(long, help = "副本的名字，默认用原名，同一层已有时叫“原名 副本”")]
        name: Option<String>,
    },
    #[command(about = "删除事项（编号）或文件夹（路径，连同里面的一切）")]
    Rm {
        #[arg(required = true)]
        items: Vec<String>,
    },
    #[command(about = "加减依赖")]
    Dep {
        #[command(subcommand)]
        command: Dep,
    },
    #[command(about = "给事项加附件，每个文件一个批次")]
    Attach {
        #[arg(value_parser = task, help = TASK)]
        task: String,
        #[arg(required = true, value_name = "文件")]
        files: Vec<PathBuf>,
        #[arg(long, help = "附件的名称，默认是文件名；只能和一个文件一起给")]
        name: Option<String>,
    },
    #[command(about = "删除附件")]
    Detach {
        #[arg(required = true, value_name = "附件 ID")]
        attachments: Vec<String>,
    },
    #[command(about = "下载附件")]
    Download {
        #[arg(value_name = "附件 ID")]
        id: String,
        #[arg(short, long, help = "存到哪个文件或目录，默认当前目录，用附件的名称")]
        output: Option<PathBuf>,
    },
    #[command(about = "把这个账号自己的数据下载成备份文件：可以在本机模式里恢复，也可以导入别的账号")]
    Export {
        #[arg(value_name = "文件", help = "存到哪个文件，默认当前目录下的 NextTodo-<用户名>-<日期>.tar")]
        file: Option<PathBuf>,
    },
    #[command(about = "把备份文件（本机模式的备份，或导出的文件）加进这个账号，原有的数据不动")]
    Import {
        #[arg(value_name = "备份文件")]
        file: PathBuf,
    },
    #[command(about = "按计划批量新建文件夹、事项和依赖")]
    Apply {
        #[arg(help = "YAML 计划文件，- 表示从标准输入读")]
        file: PathBuf,
        #[arg(long, help = "只预览，不保存")]
        dry_run: bool,
    },
    #[command(about = "把自己的文件夹共享给他人，或添加成员")]
    Share {
        folder: String,
        #[arg(required = true, value_name = "用户名")]
        users: Vec<String>,
    },
    #[command(about = "共享人移除成员，不写用户名就是停止共享；成员不写用户名就是自己退出")]
    Unshare {
        folder: String,
        #[arg(value_name = "用户名")]
        users: Vec<String>,
    },
    #[command(about = "撤销一个批次")]
    Undo { batch: i64 },
    #[command(about = "MCP 服务器（stdio），转接到 NEXTTODO_URL 的服务器；本机模式的 App 用它接入 agent")]
    Mcp,
    #[command(about = "教 agent 使用 NextTodo 的 skill")]
    Skill {
        #[command(subcommand)]
        command: Skill,
    },
}

#[derive(Subcommand)]
pub enum Skill {
    #[command(about = "装到 agent 的 skill 目录：默认是本机已有的 ~/.claude/skills、~/.codex/skills")]
    Install {
        #[arg(long, help = "skill 目录，可以给多个")]
        dir: Vec<PathBuf>,
    },
}

#[derive(Subcommand)]
pub enum Dep {
    #[command(about = "加依赖：前置完成后才能做后续")]
    Add {
        #[arg(value_parser = task)]
        before: String,
        #[arg(value_parser = task)]
        after: String,
    },
    #[command(about = "删依赖")]
    Rm {
        #[arg(value_parser = task)]
        before: String,
        #[arg(value_parser = task)]
        after: String,
    },
}

const TASK: &str = "事项编号：12 或 #12；他人共享的文件夹里的写成 @alice#42";

/// A task by number: `12`, `#12`, or `@alice#42` in a folder someone else shares.
fn task(s: &str) -> Result<String, String> {
    let number = match s.strip_prefix('@') {
        Some(rest) => rest.split_once('#').map(|(_, n)| n),
        None => Some(s.strip_prefix('#').unwrap_or(s)),
    };
    match number.map(str::parse::<i64>) {
        Some(Ok(_)) => Ok(s.to_owned()),
        _ => Err(format!("“{s}”不是事项编号")),
    }
}

/// Splits arguments into tasks and folder paths.
pub fn items(items: &[String]) -> (Vec<&str>, Vec<&str>) {
    items.iter().map(String::as_str).partition(|item| task(item).is_ok())
}
