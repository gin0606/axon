mod actor;
mod db;
mod derived;
mod domain;

use chrono::{NaiveDate, Utc};
use clap::{Parser, Subcommand};
use db::{Change, Ctx, Store};
use derived::View;
use domain::*;

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
    /// 着手可能なものから 1 件取って着手する
    Next,
    /// 指定して着手する
    Start { id: String },
    /// 終了にする
    Done {
        id: String,
        /// 履歴に残す理由
        #[arg(short, long)]
        reason: Option<String>,
    },
    /// 説明を書く (指定が無ければ $EDITOR を開く)
    Describe {
        id: String,
        /// 本文を直接渡す
        #[arg(short = 'm', long)]
        message: Option<String>,
        /// ファイルから読む (- で標準入力)
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
    if let Err(e) = run() {
        eprintln!("エラー: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init { prefix } => cmd_init(prefix),
        Command::Plan { title } => cmd_create(title, Commitment::Accepted),
        Command::Capture { title } => cmd_create(title, Commitment::Undecided),
        Command::Ready => cmd_ready(),
        Command::Triage => cmd_triage(),
        Command::Next => cmd_next(),
        Command::Start { id } => cmd_start(&id),
        Command::Done { id, reason } => cmd_done(&id, reason),
        Command::Describe { id, message, file } => cmd_describe(&id, message, file),
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
            let display = if name.is_empty() { slug.clone() } else { name.join(" ") };
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
            if let Some(pid) = &g.parent {
                if let Some(parent) = view.group(pid) {
                    println!("親: {}", parent.slug);
                }
            }
            let p = view.group_progress(&id);
            match p.ratio() {
                Some(_) => println!("進捗: {}/{} (未判断 {} 件)", p.done, p.total, p.undecided),
                None => println!("進捗: 採用したものがまだありません (未判断 {} 件)", p.undecided),
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

fn cmd_create(title: Vec<String>, commitment: Commitment) -> Result<(), Box<dyn std::error::Error>> {
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
    let ready = view.ready();
    if ready.is_empty() {
        println!("着手できるものはありません");
        return Ok(());
    }
    for i in ready {
        println!("{}  {}", i.id, i.title);
    }
    Ok(())
}

fn cmd_triage() -> Result<(), Box<dyn std::error::Error>> {
    use derived::TriageReason;
    let (_, view) = load()?;
    let items = view.triage();
    if items.is_empty() {
        println!("判断を待っているものはありません");
        return Ok(());
    }
    for (issue, reason) in items {
        match reason {
            TriageReason::Undecided => {
                println!("{}  未判断    {}", issue.id, issue.title);
            }
            TriageReason::Orphaned => {
                let lost: Vec<_> = view
                    .depends_on(&issue.id)
                    .into_iter()
                    .filter(|d| d.commitment == Commitment::Rejected)
                    .map(|d| d.id.to_string())
                    .collect();
                println!(
                    "{}  前提喪失  {} ← {} が不採用",
                    issue.id,
                    issue.title,
                    lost.join(", ")
                );
            }
        }
    }
    Ok(())
}

fn cmd_next() -> Result<(), Box<dyn std::error::Error>> {
    let mut store = Store::open()?;
    let claim = Claim {
        actor: actor::actor(),
        session: actor::session_key(),
        pid: actor::pid(),
        at: Utc::now(),
    };
    match store.claim_next(claim.clone())? {
        Some(issue) => {
            println!("{} に着手しました ({})", issue.id, claim.actor);
            println!("{}", issue.title);
        }
        None => println!("着手できるものはありません"),
    }
    Ok(())
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
    Ctx { actor: actor::actor(), reason }
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
    let mut any = false;
    for i in view.iter() {
        any = true;
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
        println!(
            "{}  [{}/{}]{}  {}",
            i.id,
            i.progress.label(),
            i.commitment.label(),
            mark,
            i.title
        );
    }
    if !any {
        println!("issue はまだありません");
    }
    Ok(())
}

fn cmd_show(id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let (store, view) = load()?;
    let id = store.resolve_id(id)?;
    let issue = view.get(&id).ok_or("issue が見つかりません")?;

    println!("{}  {}", issue.id, issue.title);
    let when = match &issue.condition {
        None => "常に浮上".to_string(),
        Some(Condition::At(d)) => format!("{d} 以降"),
        Some(Condition::AfterIssue(r)) => format!("{r} が終わった後"),
    };
    println!(
        "進行: {}  採否: {}  時期: {}",
        issue.progress.label(),
        issue.commitment.label(),
        when
    );
    if let Some(c) = issue.progress.claim() {
        println!("着手: {} ({})", c.actor, c.at.format("%Y-%m-%d %H:%M"));
    }
    if let Some(d) = &issue.description {
        println!("\n{d}");
    }

    let waiting: Vec<_> = view
        .depends_on(&id)
        .into_iter()
        .filter(|d| !d.is_terminal())
        .collect();
    if !waiting.is_empty() {
        println!();
        for d in &waiting {
            println!("待ち: {}  {}{}", d.id, d.title, blocker_note(&view, d));
        }
    }
    let lost: Vec<_> = view
        .depends_on(&id)
        .into_iter()
        .filter(|d| d.commitment == Commitment::Rejected)
        .collect();
    for d in lost {
        println!("前提喪失: {} が不採用になっています", d.id);
    }

    // 直接の依存より奥に原因があるときだけ、遡った結果を出す
    let causes = view.blocked_reason(&id);
    let direct: Vec<_> = waiting.iter().map(|d| &d.id).collect();
    for c in causes.iter().filter(|c| !direct.contains(&&c.id)) {
        println!("原因: {}  {}{}", c.id, c.title, blocker_note(&view, c));
    }

    for (g, blockers) in view.group_blocked_reason(issue) {
        println!();
        println!("グループ {} が {} を待っています", 
            issue.group.as_ref().and_then(|x| view.group(x)).map(|x| x.slug.as_str()).unwrap_or("?"),
            g.slug);
        for b in blockers {
            println!("  {}  [{}/{}]  {}", b.id, b.progress.label(), b.commitment.label(), b.title);
        }
    }
    Ok(())
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
            store.apply(&id, Change::SetCondition(Some(Condition::At(date))), &ctx(reason))?;
            println!("{id} は {date} まで浮上しません");
        }
        WhenCmd::After { id, reference, reason } => {
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

fn cmd_describe(
    id: &str,
    message: Option<String>,
    file: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Read;

    let mut store = Store::open()?;
    let id = store.resolve_id(id)?;
    let current = store.get(&id)?;

    let text = match (message, file) {
        (Some(m), None) => m,
        (None, Some(f)) if f == "-" => {
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            buf
        }
        (None, Some(f)) => std::fs::read_to_string(&f)?,
        (None, None) => edit_in_editor(current.description.as_deref())?,
        (Some(_), Some(_)) => return Err("-m と -F は同時に使えません".into()),
    };

    let trimmed = text.trim();
    let new = (!trimmed.is_empty()).then(|| trimmed.to_string());
    if new == current.description {
        println!("変更はありません");
        return Ok(());
    }
    let removed = new.is_none();
    store.apply(&id, Change::SetDescription(new), &ctx(None))?;
    if removed {
        println!("{id} の説明を消しました");
    } else {
        println!("{id} の説明を書きました");
    }
    Ok(())
}

/// $EDITOR で編集する。本文だけを出す。
/// 説明用のヘッダを混ぜると、Markdown の見出しと区別できなくなるため置かない。
fn edit_in_editor(current: Option<&str>) -> Result<String, Box<dyn std::error::Error>> {
    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .map_err(|_| "$EDITOR が設定されていません。-m か -F を使ってください")?;

    let path = std::env::temp_dir().join(format!("axon-{}.md", std::process::id()));
    std::fs::write(&path, current.unwrap_or(""))?;

    let status = std::process::Command::new(&editor).arg(&path).status()?;
    if !status.success() {
        std::fs::remove_file(&path).ok();
        return Err(format!("{editor} が異常終了しました").into());
    }
    let body = std::fs::read_to_string(&path)?;
    std::fs::remove_file(&path).ok();
    Ok(body)
}
