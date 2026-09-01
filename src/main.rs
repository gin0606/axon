mod actor;
mod db;
mod derived;
mod domain;

use chrono::{NaiveDate, Utc};
use clap::{CommandFactory, Parser, Subcommand};
use db::{Change, Ctx, Store};
use derived::View;
use domain::*;

const CLI_GUIDE: &str = include_str!("../docs/cli.md");

#[derive(Parser)]
#[command(name = "axon", version, about = "軸を分けたローカル issue tracker")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// この リポジトリで axon を使い始める
    Init {
        /// issue ID の接頭辞 (省略時はリポジトリのディレクトリ名)
        prefix: Option<String>,
    },
    /// やると決めたものを登録する (採否=採用)
    Plan { title: Vec<String> },
    /// 判断は後回しにして投げ込む (採否=未判断)
    Capture { title: Vec<String> },
    /// 着手可能なものを見る
    Ready,
    /// 人間の判断を待っているものを見る
    Triage,
    /// 指定して着手する
    Start { id: String },
    /// 終了にする
    Done {
        id: String,
        /// 履歴に残す理由
        #[arg(short, long)]
        reason: Option<String>,
    },
    /// 表題と説明を書く
    Write {
        id: String,
        /// 表題を付け直す
        #[arg(long)]
        title: Option<String>,
        /// 説明を直接渡す
        #[arg(short = 'm', long)]
        message: Option<String>,
        /// 説明をファイルから読む (- で標準入力)
        #[arg(short = 'F', long)]
        file: Option<String>,
    },
    /// 変更の履歴を見る
    Log { id: String },
    /// 放置されたまま残っている着手を探す
    Stale {
        /// これより長く動きがないものを対象にする
        #[arg(long, default_value_t = 24)]
        hours: i64,
    },
    /// 着手を取り消して未着手に戻す
    Release {
        id: String,
        #[arg(short, long)]
        reason: Option<String>,
    },
    /// 全 issue を見る
    List,
    /// 詳細を見る
    Show { id: String },
    /// 採否 (やるかどうか) を決める
    #[command(subcommand)]
    Decide(DecideCmd),
    /// 時期 (いつ再び意識に上げるか) を決める
    #[command(subcommand)]
    When(WhenCmd),
    /// 依存 (これが無いと始められない) を張る
    #[command(subcommand)]
    Dep(DepCmd),
    /// 機能群でまとめる
    #[command(subcommand)]
    Group(GroupCmd),
}

#[derive(Subcommand)]
enum GroupCmd {
    /// グループを作る
    New {
        slug: String,
        /// 表示名 (省略時は slug と同じ)
        name: Vec<String>,
        /// 親グループの slug
        #[arg(long)]
        parent: Option<String>,
    },
    /// グループを一覧する
    List,
    /// グループの中身と進捗を見る
    Show { slug: String },
    /// issue をグループに入れる
    Set { id: String, slug: String },
    /// issue をグループから外す
    Unset { id: String },
    /// グループごとやらないことにする (子孫を一括で不採用にする)
    Reject { slug: String },
    /// グループ間の依存
    #[command(subcommand)]
    Dep(GroupDepCmd),
}

#[derive(Subcommand)]
enum GroupDepCmd {
    /// 依存を張る
    Add {
        slug: String,
        #[arg(long)]
        needs: String,
    },
    /// 依存を外す
    Rm {
        slug: String,
        #[arg(long)]
        needs: String,
    },
}

#[derive(Subcommand)]
enum DecideCmd {
    /// やると決める
    Accept {
        id: String,
        #[arg(short, long)]
        reason: Option<String>,
    },
    /// やらないと決める
    Reject {
        id: String,
        /// なぜやらないのか。後から見て判断を復元できるように残す
        #[arg(short, long)]
        reason: Option<String>,
    },
    /// 判断を取り消して未判断に戻す
    Undecide {
        id: String,
        #[arg(short, long)]
        reason: Option<String>,
    },
}

#[derive(Subcommand)]
enum WhenCmd {
    /// 指定日まで浮上させない
    At {
        id: String,
        date: String,
        /// なぜ今やらないのか
        #[arg(short, long)]
        reason: Option<String>,
    },
    /// 指定した issue が終わるまで浮上させない
    After {
        id: String,
        reference: String,
        #[arg(short, long)]
        reason: Option<String>,
    },
    /// 条件を外して常に浮上させる
    Clear {
        id: String,
        #[arg(short, long)]
        reason: Option<String>,
    },
}

#[derive(Subcommand)]
enum DepCmd {
    /// 依存を張る
    Add {
        id: String,
        #[arg(long)]
        needs: String,
    },
    /// 依存を外す
    Rm {
        id: String,
        #[arg(long)]
        needs: String,
    },
}

fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    if requests_complete_help(&args) {
        if let Err(e) = write_complete_help() {
            eprintln!("エラー: {e}");
            std::process::exit(1);
        }
        return;
    }

    if let Err(e) = run(args) {
        eprintln!("エラー: {e}");
        std::process::exit(1);
    }
}

fn write_complete_help() -> std::io::Result<()> {
    use std::io::Write;

    let mut stdout = std::io::stdout().lock();
    match stdout.write_all(render_complete_help().as_bytes()) {
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        result => result,
    }
}

fn requests_complete_help(args: &[std::ffi::OsString]) -> bool {
    match args.get(1).and_then(|arg| arg.to_str()) {
        Some("--help") => true,
        Some("help") => args.len() == 3 && args.get(2).and_then(|arg| arg.to_str()) == Some("all"),
        _ => false,
    }
}

fn render_complete_help() -> String {
    let mut command = Cli::command().term_width(0);
    command.build();

    let mut out = String::new();
    out.push_str(CLI_GUIDE.trim_end());
    out.push_str("\n\n# コマンドリファレンス\n");
    write_leaf_help(&mut out, &mut command, "axon");
    out
}

fn write_leaf_help(out: &mut String, command: &mut clap::Command, path: &str) {
    let has_generated_help = !command.is_disable_help_subcommand_set();
    let has_visible_children = command
        .get_subcommands()
        .any(|child| !child.is_hide_set() && !(has_generated_help && child.get_name() == "help"));

    if !has_visible_children {
        out.push_str(&format!("\n## `{path}`\n\n"));
        out.push_str(command.render_long_help().to_string().trim_end());
        out.push('\n');
        return;
    }

    for child in command.get_subcommands_mut() {
        if child.is_hide_set() || (has_generated_help && child.get_name() == "help") {
            continue;
        }
        write_leaf_help(out, child, &format!("{path} {}", child.get_name()));
    }
}

fn run(args: Vec<std::ffi::OsString>) -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse_from(args);
    match cli.command {
        Command::Init { prefix } => cmd_init(prefix),
        Command::Plan { title } => cmd_create(title, Commitment::Accepted),
        Command::Capture { title } => cmd_create(title, Commitment::Undecided),
        Command::Ready => cmd_ready(),
        Command::Triage => cmd_triage(),
        Command::Start { id } => cmd_start(&id),
        Command::Done { id, reason } => cmd_done(&id, reason),
        Command::Write {
            id,
            title,
            message,
            file,
        } => cmd_write(&id, title, message, file),
        Command::Log { id } => cmd_log(&id),
        Command::Stale { hours } => cmd_stale(hours),
        Command::Release { id, reason } => cmd_release(&id, reason),
        Command::List => cmd_list(),
        Command::Show { id } => cmd_show(&id),
        Command::Decide(c) => cmd_decide(c),
        Command::When(c) => cmd_when(c),
        Command::Dep(c) => cmd_dep(c),
        Command::Group(c) => cmd_group(c),
    }
}

fn cmd_group(c: GroupCmd) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = Store::open()?;
    match c {
        GroupCmd::New { slug, name, parent } => {
            let parent = match parent {
                Some(p) => Some(store.resolve_slug(&p)?),
                None => None,
            };
            let display = if name.is_empty() {
                slug.clone()
            } else {
                name.join(" ")
            };
            let group = Group {
                id: GroupId::generate(),
                slug: slug.clone(),
                name: display.clone(),
                description: None,
                parent,
            };
            store.insert_group(&group)?;
            println!("{slug}  {display}");
        }
        GroupCmd::List => {
            let view = view_of(&store)?;
            if view.groups().is_empty() {
                println!("グループはまだありません");
                return Ok(());
            }
            for g in view.groups() {
                let p = view.group_progress(&g.id);
                let waiting = view.group_waiting_on(&g.id);
                let mut extra = Vec::new();
                if p.undecided > 0 {
                    extra.push(format!("未判断 {}", p.undecided));
                }
                if !waiting.is_empty() {
                    let names: Vec<_> = waiting.iter().map(|w| w.slug.as_str()).collect();
                    extra.push(format!("待ち: {}", names.join(", ")));
                }
                let suffix = if extra.is_empty() {
                    String::new()
                } else {
                    format!("  ({})", extra.join(" / "))
                };
                println!("{}  {}/{}{}  {}", g.slug, p.done, p.total, suffix, g.name);
            }
        }
        GroupCmd::Show { slug } => {
            let view = view_of(&store)?;
            let id = store.resolve_slug(&slug)?;
            let g = view.group(&id).ok_or("グループが見つかりません")?;
            println!("{}  {}", g.slug, g.name);
            if let Some(pid) = &g.parent
                && let Some(parent) = view.group(pid)
            {
                println!("親: {}", parent.slug);
            }
            let p = view.group_progress(&id);
            match p.ratio() {
                Some(_) => println!("進捗: {}/{} (未判断 {} 件)", p.done, p.total, p.undecided),
                None => println!(
                    "進捗: 採用したものがまだありません (未判断 {} 件)",
                    p.undecided
                ),
            }
            let waiting = view.group_waiting_on(&id);
            for w in waiting {
                println!("待ち: {} ({})", w.slug, w.name);
            }
            println!();
            for i in view.issues_in(&id) {
                println!(
                    "  {}  [{}/{}]  {}",
                    i.id,
                    i.progress.label(),
                    i.commitment.label(),
                    i.title
                );
            }
        }
        GroupCmd::Set { id, slug } => {
            let id = store.resolve_id(&id)?;
            let gid = store.resolve_slug(&slug)?;
            store.apply(&id, Change::SetGroup(Some(gid)), &ctx(None))?;
            println!("{id} を {slug} に入れました");
        }
        GroupCmd::Unset { id } => {
            let id = store.resolve_id(&id)?;
            store.apply(&id, Change::SetGroup(None), &ctx(None))?;
            println!("{id} をグループから外しました");
        }
        GroupCmd::Reject { slug } => {
            let view = view_of(&store)?;
            let id = store.resolve_slug(&slug)?;
            let targets: Vec<_> = view
                .issues_in(&id)
                .into_iter()
                .filter(|i| i.commitment != Commitment::Rejected)
                .map(|i| i.id.clone())
                .collect();
            let c = ctx(Some(format!("{slug} ごと不採用にした")));
            for t in &targets {
                store.apply(t, Change::Decide(Commitment::Rejected), &c)?;
            }
            println!("{slug} の {} 件を不採用にしました", targets.len());
        }
        GroupCmd::Dep(d) => match d {
            GroupDepCmd::Add { slug, needs } => {
                let a = store.resolve_slug(&slug)?;
                let b = store.resolve_slug(&needs)?;
                if a == b {
                    return Err("自分自身には依存できません".into());
                }
                store.add_group_dep(&a, &b)?;
                println!("{slug} は {needs} を前提にします");
            }
            GroupDepCmd::Rm { slug, needs } => {
                let a = store.resolve_slug(&slug)?;
                let b = store.resolve_slug(&needs)?;
                store.remove_group_dep(&a, &b)?;
                println!("{slug} の前提から {needs} を外しました");
            }
        },
    }
    Ok(())
}

fn cmd_init(prefix: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let prefix = match prefix {
        Some(p) => p,
        None => std::env::current_dir()?
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "axon".to_string()),
    };
    let path = Store::init(&prefix)?;
    println!("初期化しました: {}", path.display());
    println!("issue ID は {prefix}-xxxxxx の形式になります");
    Ok(())
}

fn cmd_create(
    title: Vec<String>,
    commitment: Commitment,
) -> Result<(), Box<dyn std::error::Error>> {
    let title = title.join(" ");
    if title.trim().is_empty() {
        return Err("タイトルが空です".into());
    }
    let store = Store::open()?;
    let now = Utc::now();
    let issue = Issue {
        id: IssueId::generate(&store.prefix()?),
        title,
        description: None,
        progress: Progress::NotStarted,
        commitment,
        condition: None,
        group: None,
        created_at: now,
        updated_at: now,
    };
    store.insert(&issue)?;
    println!("{}  {}  [{}]", issue.id, issue.title, commitment.label());
    Ok(())
}

fn load() -> Result<(Store, View), Box<dyn std::error::Error>> {
    let store = Store::open()?;
    let view = view_of(&store)?;
    Ok((store, view))
}

fn view_of(store: &Store) -> Result<View, Box<dyn std::error::Error>> {
    Ok(View::new(
        store.all()?,
        store.deps()?,
        store.groups()?,
        store.group_deps()?,
    ))
}

fn cmd_ready() -> Result<(), Box<dyn std::error::Error>> {
    let (_, view) = load()?;
    print_rows(&render_ready(&view), "着手できるものはありません");
    Ok(())
}

/// 案内を標準出力に混ぜると `axon ready | fzf` でその 1 行が選択肢になる。
fn print_rows(rows: &str, empty_note: &str) {
    if rows.is_empty() {
        eprintln!("{empty_note}");
        return;
    }
    print!("{rows}");
}

fn render_ready(view: &View) -> String {
    view.ready()
        .into_iter()
        .map(|i| format!("{}  {}\n", i.id, i.title))
        .collect()
}

fn cmd_triage() -> Result<(), Box<dyn std::error::Error>> {
    let (_, view) = load()?;
    print_rows(&render_triage(&view), "判断を待っているものはありません");
    Ok(())
}

fn render_triage(view: &View) -> String {
    use derived::TriageReason;
    let mut out = String::new();
    for (issue, reason) in view.triage() {
        match reason {
            TriageReason::Undecided => {
                out.push_str(&format!("{}  未判断    {}\n", issue.id, issue.title));
            }
            TriageReason::Orphaned => {
                let lost: Vec<_> = view
                    .depends_on(&issue.id)
                    .into_iter()
                    .filter(|d| d.commitment == Commitment::Rejected)
                    .map(|d| d.id.to_string())
                    .collect();
                out.push_str(&format!(
                    "{}  前提喪失  {} ← {} が不採用\n",
                    issue.id,
                    issue.title,
                    lost.join(", ")
                ));
            }
        }
    }
    out
}

fn cmd_start(id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = Store::open()?;
    let id = store.resolve_id(id)?;
    let claim = Claim {
        actor: actor::actor(),
        session: actor::session_key(),
        pid: actor::pid(),
        at: Utc::now(),
    };
    store.apply(&id, Change::Claim(claim.clone()), &ctx(None))?;
    let issue = store.get(&id)?;
    println!("{} に着手しました ({})", id, claim.actor);
    println!("{}", issue.title);
    Ok(())
}

fn ctx(reason: Option<String>) -> Ctx {
    Ctx {
        actor: actor::actor(),
        reason,
    }
}

fn cmd_done(id: &str, reason: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = Store::open()?;
    let id = store.resolve_id(id)?;
    store.apply(&id, Change::End, &ctx(reason))?;
    println!("{id} を終了しました");

    let view = view_of(&store)?;
    let unblocked = view.newly_ready_after(&id);
    for i in unblocked {
        println!("着手可能になりました: {}  {}", i.id, i.title);
    }
    Ok(())
}

fn cmd_list() -> Result<(), Box<dyn std::error::Error>> {
    let (_, view) = load()?;
    print_rows(&render_list(&view), "issue はまだありません");
    Ok(())
}

fn render_list(view: &View) -> String {
    let mut out = String::new();
    for i in view.iter() {
        let mut marks = Vec::new();
        if view.is_orphaned(&i.id) {
            marks.push("前提喪失".to_string());
        } else if view.is_blocked(&i.id) {
            marks.push("待ち".to_string());
        }
        if !view.is_surfaced(i) {
            marks.push(match &i.condition {
                Some(Condition::At(d)) => d.to_string(),
                Some(Condition::AfterIssue(r)) => format!("{r} の後"),
                None => String::new(),
            });
        }
        let mark = if marks.is_empty() {
            String::new()
        } else {
            format!(" {}", marks.join(" "))
        };
        out.push_str(&format!(
            "{}  [{}/{}]{}  {}\n",
            i.id,
            i.progress.label(),
            i.commitment.label(),
            mark,
            i.title
        ));
    }
    out
}

fn cmd_show(id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let (store, view) = load()?;
    let id = store.resolve_id(id)?;
    let issue = view.get(&id).ok_or("issue が見つかりません")?;
    print!("{}", render_show(&view, issue));
    Ok(())
}

/// show の本文。該当が無い区分で空の見出しを出さないため、行の無いブロックは落とす。
fn render_show(view: &View, issue: &Issue) -> String {
    let mut blocks: Vec<Vec<String>> = Vec::new();

    let when = match &issue.condition {
        None => "常に浮上".to_string(),
        Some(Condition::At(d)) => format!("{d} 以降"),
        Some(Condition::AfterIssue(r)) => format!("{r} が終わった後"),
    };
    let mut head = vec![
        format!("{}  {}", issue.id, issue.title),
        format!(
            "進行: {}  採否: {}  時期: {}",
            issue.progress.label(),
            issue.commitment.label(),
            when
        ),
    ];
    let group = issue.group.as_ref().and_then(|g| view.group(g));
    if let Some(g) = group {
        head.push(if g.name == g.slug {
            format!("グループ: {}", g.slug)
        } else {
            format!("グループ: {}  {}", g.slug, g.name)
        });
    }
    if let Some(c) = issue.progress.claim() {
        head.push(format!(
            "着手: {} ({})",
            c.actor,
            c.at.format("%Y-%m-%d %H:%M")
        ));
    }
    blocks.push(head);

    if let Some(d) = &issue.description {
        blocks.push(vec![d.clone()]);
    }

    let deps = view.depends_on(&issue.id);
    let waiting: Vec<&Issue> = deps.iter().copied().filter(|d| !d.is_terminal()).collect();
    let mut relations = Vec::new();
    for d in &waiting {
        relations.push(format!(
            "待ち: {}  {}{}",
            d.id,
            d.title,
            blocker_note(view, d)
        ));
    }
    for d in deps
        .iter()
        .filter(|d| d.is_terminal() && d.commitment != Commitment::Rejected)
    {
        relations.push(format!("済み: {}  {}", d.id, d.title));
    }
    for d in deps.iter().filter(|d| d.commitment == Commitment::Rejected) {
        relations.push(format!("前提喪失: {} が不採用になっています", d.id));
    }

    // 直接の依存より奥に原因があるときだけ、遡った結果を出す
    let direct: Vec<&IssueId> = waiting.iter().map(|d| &d.id).collect();
    for c in view
        .blocked_reason(&issue.id)
        .iter()
        .filter(|c| !direct.contains(&&c.id))
    {
        relations.push(format!(
            "原因: {}  {}{}",
            c.id,
            c.title,
            blocker_note(view, c)
        ));
    }

    for d in view.dependents(&issue.id) {
        relations.push(format!("後続: {}  {}{}", d.id, d.title, dependent_note(d)));
    }
    blocks.push(relations);

    for (g, blockers) in view.group_blocked_reason(issue) {
        let mut block = vec![format!(
            "グループ {} が {} を待っています",
            group.map(|x| x.slug.as_str()).unwrap_or("?"),
            g.slug
        )];
        for b in blockers {
            block.push(format!(
                "  {}  [{}/{}]  {}",
                b.id,
                b.progress.label(),
                b.commitment.label(),
                b.title
            ));
        }
        blocks.push(block);
    }

    let body = blocks
        .into_iter()
        .filter(|b| !b.is_empty())
        .map(|b| b.join("\n"))
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("{body}\n")
}

/// 後続がすでに終端なら、これを終わらせても動き出さないことを添える。
fn dependent_note(issue: &Issue) -> &'static str {
    if issue.commitment == Commitment::Rejected {
        " ← 不採用"
    } else if matches!(issue.progress, Progress::Ended) {
        " ← 終了済み"
    } else {
        ""
    }
}

/// 止まっている理由のうち、状態表示だけでは読み取れないものを添える。
fn blocker_note(view: &View, issue: &Issue) -> String {
    if view.is_orphaned(&issue.id) {
        " ← 前提喪失".to_string()
    } else if !view.is_surfaced(issue) {
        match &issue.condition {
            Some(Condition::At(d)) => format!(" ← {d} まで浮上しない"),
            Some(Condition::AfterIssue(r)) => format!(" ← {r} の後まで浮上しない"),
            None => String::new(),
        }
    } else {
        String::new()
    }
}

fn cmd_decide(c: DecideCmd) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = Store::open()?;
    let (raw, commitment, reason) = match c {
        DecideCmd::Accept { id, reason } => (id, Commitment::Accepted, reason),
        DecideCmd::Reject { id, reason } => (id, Commitment::Rejected, reason),
        DecideCmd::Undecide { id, reason } => (id, Commitment::Undecided, reason),
    };
    let id = store.resolve_id(&raw)?;
    let before = store.get(&id)?;
    store.apply(&id, Change::Decide(commitment), &ctx(reason))?;
    println!("{id} を{}にしました", commitment.label());

    // 着手中のまま不採用にすると「作業は止まっているのに着手中」が残るため促す。
    // 進行と採否は別の軸なので、こちらでは終了させない。
    if commitment == Commitment::Rejected && before.progress.claim().is_some() {
        println!("着手中のままです。作業を止めるなら `axon done {id}` も実行してください");
    }
    Ok(())
}

fn cmd_when(c: WhenCmd) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = Store::open()?;
    match c {
        WhenCmd::At { id, date, reason } => {
            let id = store.resolve_id(&id)?;
            let date: NaiveDate = date
                .parse()
                .map_err(|_| format!("日付として読めません: {date} (YYYY-MM-DD)"))?;
            store.apply(
                &id,
                Change::SetCondition(Some(Condition::At(date))),
                &ctx(reason),
            )?;
            println!("{id} は {date} まで浮上しません");
        }
        WhenCmd::After {
            id,
            reference,
            reason,
        } => {
            let id = store.resolve_id(&id)?;
            let target = store.resolve_id(&reference)?;
            if id == target {
                return Err("自分自身を条件にはできません".into());
            }
            store.apply(
                &id,
                Change::SetCondition(Some(Condition::AfterIssue(target.clone()))),
                &ctx(reason),
            )?;
            println!("{id} は {target} が終わるまで浮上しません");
        }
        WhenCmd::Clear { id, reason } => {
            let id = store.resolve_id(&id)?;
            store.apply(&id, Change::SetCondition(None), &ctx(reason))?;
            println!("{id} の条件を外しました");
        }
    }
    Ok(())
}

fn cmd_dep(c: DepCmd) -> Result<(), Box<dyn std::error::Error>> {
    let store = Store::open()?;
    match c {
        DepCmd::Add { id, needs } => {
            let id = store.resolve_id(&id)?;
            let needs = store.resolve_id(&needs)?;
            if id == needs {
                return Err("自分自身には依存できません".into());
            }
            store.add_dep(&id, &needs)?;
            println!("{id} は {needs} を前提にします");
        }
        DepCmd::Rm { id, needs } => {
            let id = store.resolve_id(&id)?;
            let needs = store.resolve_id(&needs)?;
            store.remove_dep(&id, &needs)?;
            println!("{id} の前提から {needs} を外しました");
        }
    }
    Ok(())
}

fn cmd_log(id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let store = Store::open()?;
    let id = store.resolve_id(id)?;
    let events = store.events(&id)?;
    if events.is_empty() {
        println!("履歴はありません");
        return Ok(());
    }
    for e in events {
        print!(
            "{}  {}  {}",
            e.at.format("%Y-%m-%d %H:%M"),
            e.actor,
            format_decision(&e)
        );
        match e.reason {
            Some(r) => println!("  ({r})"),
            None => println!(),
        }
    }
    Ok(())
}

/// 判断を日本語の一行にする。DB には英語の値が入っているため、ここで読める形に直す。
fn format_decision(e: &db::Event) -> String {
    let label = |v: &Option<String>| -> String {
        match v.as_deref() {
            Some("undecided") => "未判断".to_string(),
            Some("accepted") => "採用".to_string(),
            Some("rejected") => "不採用".to_string(),
            Some(other) => other.to_string(),
            None => "なし".to_string(),
        }
    };
    match e.field.as_str() {
        "commitment" => format!("採否: {} → {}", label(&e.old_value), label(&e.new_value)),
        "condition" => match (&e.old_value, &e.new_value) {
            (_, None) => "時期: 条件を外した".to_string(),
            (None, Some(n)) => format!("時期: {n} まで後回し"),
            (Some(o), Some(n)) => format!("時期: {o} → {n} まで後回し"),
        },
        other => format!("{other}: {} → {}", label(&e.old_value), label(&e.new_value)),
    }
}

fn cmd_stale(hours: i64) -> Result<(), Box<dyn std::error::Error>> {
    let (_, view) = load()?;
    let stale = view.stale_claims(hours);
    if stale.is_empty() {
        println!("放置された着手はありません");
        return Ok(());
    }
    for (issue, claim) in stale {
        let elapsed = chrono::Utc::now()
            .signed_duration_since(claim.at)
            .num_hours();
        println!("{}  {}", issue.id, issue.title);
        println!(
            "  {} が {} 時間前に着手 (プロセス {} は終了しています)",
            claim.actor, elapsed, claim.pid
        );
        println!("  解放するなら: axon release {}", issue.id);
    }
    Ok(())
}

fn cmd_release(id: &str, reason: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = Store::open()?;
    let id = store.resolve_id(id)?;
    let before = store.get(&id)?;
    let Some(claim) = before.progress.claim() else {
        return Err(format!("{id} は着手されていません").into());
    };
    let who = claim.actor.clone();
    store.apply(&id, Change::Release, &ctx(reason))?;
    println!("{id} の着手 ({who}) を取り消しました");
    Ok(())
}

fn cmd_write(
    id: &str,
    title: Option<String>,
    message: Option<String>,
    file: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Read;

    let mut store = Store::open()?;
    let id = store.resolve_id(id)?;
    let current = store.get(&id)?;

    // 書き込みの前に入力を出し切る。ファイルが読めずに
    // 表題だけ書き換わった状態を残さないため。
    let body = match (message, file) {
        (Some(m), None) => Some(m),
        (None, Some(f)) if f == "-" => {
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            Some(buf)
        }
        (None, Some(f)) => Some(std::fs::read_to_string(&f)?),
        (Some(_), Some(_)) => return Err("-m と -F は同時に使えません".into()),
        (None, None) => None,
    };

    if title.is_none() && body.is_none() {
        return Err("書く内容がありません。--title / -m / -F のいずれかを指定してください".into());
    }

    let mut written = Vec::new();

    if let Some(t) = title {
        let t = t.trim();
        if t.is_empty() {
            return Err("タイトルが空です".into());
        }
        if t != current.title {
            store.apply(&id, Change::SetTitle(t.to_string()), &ctx(None))?;
            written.push("表題を付け直しました");
        }
    }

    if let Some(text) = body {
        let trimmed = text.trim();
        let new = (!trimmed.is_empty()).then(|| trimmed.to_string());
        if new != current.description {
            let removed = new.is_none();
            store.apply(&id, Change::SetDescription(new), &ctx(None))?;
            written.push(if removed {
                "説明を消しました"
            } else {
                "説明を書きました"
            });
        }
    }

    if written.is_empty() {
        println!("変更はありません");
    }
    for w in written {
        println!("{id} の{w}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf_paths(command: &mut clap::Command, path: &str, out: &mut Vec<String>) {
        let has_generated_help = !command.is_disable_help_subcommand_set();
        let has_visible_children = command.get_subcommands().any(|child| {
            !child.is_hide_set() && !(has_generated_help && child.get_name() == "help")
        });

        if !has_visible_children {
            out.push(path.to_string());
            return;
        }

        for child in command.get_subcommands_mut() {
            if child.is_hide_set() || (has_generated_help && child.get_name() == "help") {
                continue;
            }
            let child_path = format!("{path} {}", child.get_name());
            leaf_paths(child, &child_path, out);
        }
    }

    fn iid(id: &str) -> IssueId {
        IssueId::from_stored(id)
    }

    fn issue(id: &str, progress: Progress, commitment: Commitment) -> Issue {
        let now = Utc::now();
        Issue {
            id: iid(id),
            title: format!("{id} の作業"),
            description: None,
            progress,
            commitment,
            condition: None,
            group: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn accepted(id: &str) -> Issue {
        issue(id, Progress::NotStarted, Commitment::Accepted)
    }

    fn done(id: &str) -> Issue {
        issue(id, Progress::Ended, Commitment::Accepted)
    }

    fn rejected(id: &str) -> Issue {
        issue(id, Progress::NotStarted, Commitment::Rejected)
    }

    fn group(slug: &str, name: &str) -> Group {
        Group {
            id: GroupId::from_stored(slug),
            slug: slug.to_string(),
            name: name.to_string(),
            description: None,
            parent: None,
        }
    }

    fn view(issues: Vec<Issue>, deps: &[(&str, &str)], groups: Vec<Group>) -> View {
        View::new(
            issues,
            deps.iter().map(|(a, b)| (iid(a), iid(b))).collect(),
            groups,
            Vec::new(),
        )
    }

    fn show(view: &View, id: &str) -> String {
        render_show(view, view.get(&iid(id)).unwrap())
    }

    fn undecided(id: &str) -> Issue {
        issue(id, Progress::NotStarted, Commitment::Undecided)
    }

    fn waiting_until(id: &str, condition: Condition) -> Issue {
        let mut i = accepted(id);
        i.condition = Some(condition);
        i
    }

    /// 一覧の各行から、空白区切りの 1 列目を取る。
    ///
    /// fzf 連携 (`axon ready | fzf --preview 'axon show {1}'`) は id をこの位置から取る。
    /// list は状態次第で列 (待ち / 前提喪失 / 浮上日) が増減するので、id を後ろに置くと
    /// 行ごとに位置がずれて preview が id を拾えなくなる。1 列目に固定しておく。
    fn first_columns(out: &str) -> Vec<&str> {
        out.lines()
            .map(|l| l.split_whitespace().next().unwrap_or(""))
            .collect()
    }

    #[test]
    fn show_without_relations_is_state_only() {
        let v = view(vec![accepted("a")], &[], Vec::new());
        assert_eq!(
            show(&v, "a"),
            "a  a の作業\n進行: 未着手  採否: 採用  時期: 常に浮上\n"
        );
    }

    #[test]
    fn show_lists_resolved_dependencies() {
        let v = view(
            vec![accepted("a"), done("b"), accepted("c")],
            &[("a", "b"), ("a", "c")],
            Vec::new(),
        );
        let out = show(&v, "a");
        assert!(out.contains("待ち: c  c の作業"), "{out}");
        assert!(out.contains("済み: b  b の作業"), "{out}");
    }

    #[test]
    fn rejected_dependency_is_not_listed_as_resolved() {
        let v = view(
            vec![
                accepted("a"),
                issue("b", Progress::Ended, Commitment::Rejected),
            ],
            &[("a", "b")],
            Vec::new(),
        );
        let out = show(&v, "a");
        assert!(out.contains("前提喪失: b が不採用になっています"), "{out}");
        assert!(!out.contains("済み:"), "{out}");
    }

    #[test]
    fn show_lists_dependents() {
        let v = view(
            vec![accepted("a"), accepted("b"), done("c"), rejected("d")],
            &[("b", "a"), ("c", "a"), ("d", "a")],
            Vec::new(),
        );
        let out = show(&v, "a");
        assert!(out.contains("後続: b  b の作業\n"), "{out}");
        assert!(out.contains("後続: c  c の作業 ← 終了済み"), "{out}");
        assert!(out.contains("後続: d  d の作業 ← 不採用"), "{out}");
    }

    #[test]
    fn show_lists_group() {
        let mut i = accepted("a");
        i.group = Some(GroupId::from_stored("cli"));
        let v = view(vec![i], &[], vec![group("cli", "CLI")]);
        assert!(
            show(&v, "a").contains("グループ: cli  CLI"),
            "{}",
            show(&v, "a")
        );
    }

    #[test]
    fn ready_puts_id_in_the_first_column() {
        let v = view(vec![accepted("a"), accepted("b")], &[], Vec::new());
        assert_eq!(first_columns(&render_ready(&v)), ["a", "b"]);
    }

    #[test]
    fn triage_puts_id_in_the_first_column() {
        let v = view(
            vec![undecided("a"), accepted("b"), rejected("c")],
            &[("b", "c")],
            Vec::new(),
        );
        assert_eq!(first_columns(&render_triage(&v)), ["a", "b"]);
    }

    #[test]
    fn list_puts_id_in_the_first_column() {
        let v = view(
            vec![
                accepted("a"),
                accepted("blocked"),
                accepted("orphaned"),
                waiting_until("dated", Condition::At(NaiveDate::MAX)),
                waiting_until("after", Condition::AfterIssue(iid("blocked"))),
                accepted("dep"),
                rejected("gone"),
            ],
            &[("blocked", "dep"), ("orphaned", "gone")],
            Vec::new(),
        );
        assert_eq!(
            first_columns(&render_list(&v)),
            ["a", "blocked", "orphaned", "dated", "after", "dep", "gone"]
        );
    }

    #[test]
    fn group_line_omits_name_equal_to_slug() {
        let mut i = accepted("a");
        i.group = Some(GroupId::from_stored("cli"));
        let v = view(vec![i], &[], vec![group("cli", "cli")]);
        assert!(
            show(&v, "a").contains("グループ: cli\n"),
            "{}",
            show(&v, "a")
        );
    }

    #[test]
    fn complete_help_embeds_the_cli_guide_verbatim() {
        assert!(render_complete_help().starts_with(CLI_GUIDE.trim_end()));
    }

    #[test]
    fn complete_help_contains_every_visible_leaf_once() {
        let mut command = Cli::command();
        command.build();
        let mut paths = Vec::new();
        leaf_paths(&mut command, "axon", &mut paths);

        let help = render_complete_help();
        for path in &paths {
            let heading = format!("## `{path}`\n");
            assert_eq!(help.matches(&heading).count(), 1, "{path}");
        }
        assert_eq!(help.matches("## `axon ").count(), paths.len());
    }

    #[test]
    fn only_root_long_help_and_help_all_request_the_complete_reference() {
        let args = |values: &[&str]| {
            values
                .iter()
                .map(std::ffi::OsString::from)
                .collect::<Vec<_>>()
        };

        assert!(requests_complete_help(&args(&["axon", "--help"])));
        assert!(requests_complete_help(&args(&["axon", "help", "all"])));
        assert!(!requests_complete_help(&args(&["axon", "-h"])));
        assert!(!requests_complete_help(&args(&[
            "axon", "decide", "reject", "--help"
        ])));
        assert!(!requests_complete_help(&args(&[
            "axon", "help", "decide", "reject"
        ])));
    }
}
