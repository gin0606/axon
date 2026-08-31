mod actor;
mod db;
mod derived;
mod domain;

use chrono::{NaiveDate, Utc};
use clap::{Parser, Subcommand};
use db::{Change, Store};
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
    /// 着手可能なものから 1 件取って着手する
    Next,
    /// 指定して着手する
    Start { id: String },
    /// 終了にする
    Done { id: String },
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
}

#[derive(Subcommand)]
enum DecideCmd {
    /// やると決める
    Accept { id: String },
    /// やらないと決める
    Reject { id: String },
    /// 判断を取り消して未判断に戻す
    Undecide { id: String },
}

#[derive(Subcommand)]
enum WhenCmd {
    /// 指定日まで浮上させない
    At { id: String, date: String },
    /// 指定した issue が終わるまで浮上させない
    After { id: String, reference: String },
    /// 条件を外して常に浮上させる
    Clear { id: String },
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
        Command::Next => cmd_next(),
        Command::Start { id } => cmd_start(&id),
        Command::Done { id } => cmd_done(&id),
        Command::List => cmd_list(),
        Command::Show { id } => cmd_show(&id),
        Command::Decide(c) => cmd_decide(c),
        Command::When(c) => cmd_when(c),
        Command::Dep(c) => cmd_dep(c),
    }
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
        created_at: now,
        updated_at: now,
    };
    store.insert(&issue)?;
    println!("{}  {}  [{}]", issue.id, issue.title, commitment.label());
    Ok(())
}

fn load() -> Result<(Store, View), Box<dyn std::error::Error>> {
    let store = Store::open()?;
    let view = View::new(store.all()?, store.deps()?);
    Ok((store, view))
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
    let store = Store::open()?;
    let id = store.resolve_id(id)?;
    let claim = Claim {
        actor: actor::actor(),
        session: actor::session_key(),
        pid: actor::pid(),
        at: Utc::now(),
    };
    store.apply(&id, Change::Claim(claim.clone()))?;
    let issue = store.get(&id)?;
    println!("{} に着手しました ({})", id, claim.actor);
    println!("{}", issue.title);
    Ok(())
}

fn cmd_done(id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let store = Store::open()?;
    let id = store.resolve_id(id)?;
    store.apply(&id, Change::End)?;
    println!("{id} を終了しました");

    let view = View::new(store.all()?, store.deps()?);
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
        for d in waiting {
            println!("待ち: {}  {}", d.id, d.title);
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
    Ok(())
}

fn cmd_decide(c: DecideCmd) -> Result<(), Box<dyn std::error::Error>> {
    let store = Store::open()?;
    let (raw, commitment) = match &c {
        DecideCmd::Accept { id } => (id, Commitment::Accepted),
        DecideCmd::Reject { id } => (id, Commitment::Rejected),
        DecideCmd::Undecide { id } => (id, Commitment::Undecided),
    };
    let id = store.resolve_id(raw)?;
    let before = store.get(&id)?;
    store.apply(&id, Change::Decide(commitment))?;
    println!("{id} を{}にしました", commitment.label());

    // 着手中のまま不採用にすると「作業は止まっているのに着手中」が残るため促す。
    // 進行と採否は別の軸なので、こちらでは終了させない。
    if commitment == Commitment::Rejected && before.progress.claim().is_some() {
        println!("着手中のままです。作業を止めるなら `axon done {id}` も実行してください");
    }
    Ok(())
}

fn cmd_when(c: WhenCmd) -> Result<(), Box<dyn std::error::Error>> {
    let store = Store::open()?;
    match c {
        WhenCmd::At { id, date } => {
            let id = store.resolve_id(&id)?;
            let date: NaiveDate = date
                .parse()
                .map_err(|_| format!("日付として読めません: {date} (YYYY-MM-DD)"))?;
            store.apply(&id, Change::SetCondition(Some(Condition::At(date))))?;
            println!("{id} は {date} まで浮上しません");
        }
        WhenCmd::After { id, reference } => {
            let id = store.resolve_id(&id)?;
            let target = store.resolve_id(&reference)?;
            if id == target {
                return Err("自分自身を条件にはできません".into());
            }
            store.apply(&id, Change::SetCondition(Some(Condition::AfterIssue(target.clone()))))?;
            println!("{id} は {target} が終わるまで浮上しません");
        }
        WhenCmd::Clear { id } => {
            let id = store.resolve_id(&id)?;
            store.apply(&id, Change::SetCondition(None))?;
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
