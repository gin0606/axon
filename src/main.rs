mod condition;
mod display;

use axon::{
    lifecycle::*,
    location::Location,
    sqlite::{self, Result},
};
use chrono::Utc;
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::{
    collections::BTreeSet,
    io::{Read, Write},
    path::PathBuf,
    time::Duration,
};

#[derive(Parser)]
#[command(
    version,
    about = "Issue と Group を単一 lifecycle で管理するローカル tracker",
    after_help = "新しい保存先で axon init → axon plan --title '仕事' → axon start ID → axon done ID。\n導入: docs/guide/getting-started.md。記録者は環境から任意取得し、log / note list --recorder-details で詳細確認。\n旧 schema の自動移行は行いません。Group の done は計画全体の最終確認済みという明示入力です。"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// snapshot を統合する（入力の保全・検査・適用）
    Merge {
        #[command(subcommand)]
        command: Merge,
    },
    /// 保存形式と全snapshotを検査する（条件は実行しない）
    Storage {
        #[command(subcommand)]
        command: Storage,
    },
    /// 新しい保存先を初期化する（既定 SQLite）
    Init {
        prefix: Option<String>,
        #[arg(long, value_enum, default_value = "sqlite")]
        backend: Backend,
    },
    /// 未判断の Issue を登録する
    Capture(Create),
    /// 採用済みの Issue を登録する
    Plan(Create),
    /// Group の登録と所属変更
    Group {
        #[command(subcommand)]
        command: Group,
    },
    /// 保存済み Entity を作成日時順に表示する（外部条件は評価しない）
    List,
    /// 自身と祖先が浮上した未判断を表示する
    Triage(CandidateOptions),
    /// 浮上した未着手（依存・親の着手待ちを含む）と全着手中を表示する
    Tasks(CandidateOptions),
    /// 再浮上条件を設定・修復する（コマンドは実行しない）
    When {
        #[command(subcommand)]
        command: When,
    },
    /// 本文と直接の待ち理由、Group の直属の子を表示する
    Show { id: String },
    /// Note を追加または取得する
    Note {
        #[command(subcommand)]
        command: Notes,
    },
    /// 状態変更と統合の経緯を表示する
    Log {
        id: String,
        /// 保存済みの記録者 data を JSON で併記する
        #[arg(long)]
        recorder_details: bool,
    },
    /// 未判断を採用する
    Accept(Change),
    /// 未着手の採用を撤回する
    Withdraw(Change),
    /// 着手する（親・依存の前提を検査する）
    Start(Change),
    /// 着手を解放する
    Release(Change),
    /// 完了する。Group では計画全体を最終確認済みであることを表す
    Done(Change),
    /// 取りやめる
    Cancel(Change),
    /// 取りやめを未判断へ戻す
    Reconsider(Change),
    /// タイトル・本文を編集する（状態は変えない）
    Write {
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[command(flatten)]
        body: Body,
    },
    /// 明示した依存先を追加・解除する
    Dep {
        #[command(subcommand)]
        command: Dependency,
    },
}
#[derive(Subcommand)]
enum Storage {
    Check { snapshot: PathBuf },
}
#[derive(Subcommand)]
#[command(
    after_help = "例: axon merge prepare --base base.jsonl --ours ours.jsonl --theirs theirs.jsonl --output .axon/state.jsonl --workspace .axon/merge-review\nresolution.json の choices を編集し、axon merge check WORKSPACE → axon merge apply WORKSPACE。\nGit driver 設定・stage・commit は利用者が行います。詳細: docs/development/lifecycle-file.md"
)]
enum Merge {
    /// 入力を保存し、衝突と解決ファイルを用意する。正本は変更しない
    Prepare {
        #[arg(long)]
        base: PathBuf,
        #[arg(long)]
        ours: PathBuf,
        #[arg(long)]
        theirs: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        workspace: PathBuf,
    },
    /// 解決案と候補全体を検査する
    Check { workspace: PathBuf },
    /// 検査済みの入力・出力を再照合して正本へ公開する
    Apply { workspace: PathBuf },
    /// Git merge driver: %O %A %B。衝突時は非0で終了し、%Aを保持する
    Driver {
        base: PathBuf,
        ours: PathBuf,
        theirs: PathBuf,
    },
}
#[derive(Args)]
#[command(
    after_help = "例: axon tasks --condition-timeout 5 --trace-conditions\n条件は /bin/sh -c で実行し、0 は成立、1 は未成立、その他は一覧取得の失敗。\n作業場所は現在の Git worktree のルート（Git 外では管理ルート）。\nCtrl-C・timeout は子を含む process group へ TERM、1秒後も残れば KILL。\nstdout/stderr は各64 KiBまで保持し、超過時は先頭・末尾32 KiBを表示します。"
)]
struct CandidateOptions {
    /// 外部コマンド1件のタイムアウト秒数（正の有限値）
    #[arg(long, default_value = "30", value_parser = parse_timeout)]
    condition_timeout: Duration,
    /// 実行した条件の結果・stdout/stderrをstderrへ表示する
    #[arg(long)]
    trace_conditions: bool,
}
fn parse_timeout(value: &str) -> std::result::Result<Duration, String> {
    let seconds: f64 = value
        .parse()
        .map_err(|_| "expected positive finite seconds")?;
    let duration =
        Duration::try_from_secs_f64(seconds).map_err(|_| "expected positive finite seconds")?;
    if duration.is_zero() {
        return Err("expected positive finite seconds".into());
    }
    Ok(duration)
}
#[derive(Subcommand)]
enum When {
    /// シェル文字列を保存・置換する（保存時に評価しない）
    #[command(
        after_help = "例: axon when set ID --command 'test -f ready.txt'\n終了0=成立、1=未成立、その他=判定失敗。壊れた条件も set / clear で修復できます。"
    )]
    Set {
        id: String,
        #[arg(long)]
        command: String,
    },
    /// 条件を解除して常に浮上させる（lifecycleは変えない）
    Clear { id: String },
}
#[derive(Clone, Copy, ValueEnum)]
enum Backend {
    Sqlite,
    File,
}
#[derive(Args)]
struct Body {
    #[arg(short = 'm', long, conflicts_with = "description_file")]
    description: Option<String>,
    /// UTF-8 本文ファイル。- は標準入力
    #[arg(short = 'F', long)]
    description_file: Option<PathBuf>,
}
impl Body {
    fn read(self) -> Result<Option<String>> {
        match self.description_file {
            Some(path) if path.as_os_str() == "-" => {
                let mut text = String::new();
                std::io::stdin().read_to_string(&mut text)?;
                Ok(Some(text))
            }
            Some(path) => Ok(Some(std::fs::read_to_string(path)?)),
            None => Ok(self.description),
        }
    }
}
#[derive(Args)]
struct Create {
    #[arg(long)]
    title: String,
    #[command(flatten)]
    body: Body,
    #[arg(long)]
    parent: Option<String>,
    #[arg(long)]
    needs: Vec<String>,
}
#[derive(Args)]
struct Change {
    id: String,
    #[arg(short, long)]
    reason: Option<String>,
}
#[derive(Subcommand)]
enum Group {
    Capture(Create),
    Plan(Create),
    Set {
        id: String,
        #[arg(long)]
        parent: String,
    },
    Unset {
        id: String,
    },
}
#[derive(Subcommand)]
enum Dependency {
    Add {
        id: String,
        #[arg(long)]
        needs: String,
    },
    Rm {
        id: String,
        #[arg(long)]
        needs: String,
    },
}
#[derive(Subcommand)]
enum Notes {
    List {
        id: String,
        /// 保存済みの記録者 data を JSON で併記する
        #[arg(long)]
        recorder_details: bool,
    },
    Add {
        id: String,
        #[arg(
            short = 'm',
            long,
            required_unless_present = "file",
            conflicts_with = "file"
        )]
        message: Option<String>,
        #[arg(short = 'F', long)]
        file: Option<PathBuf>,
    },
}
fn id(value: String) -> Result<EntityId> {
    Ok(value.try_into()?)
}
fn context() -> Context {
    Context {
        at: Utc::now(),
        recorder: axon_recorder::detect().map(|recorder| Recorder {
            actor: recorder.actor,
            data: recorder
                .data
                .into_iter()
                .map(|(key, value)| (key, value.into()))
                .collect(),
        }),
    }
}
fn row(snapshot: &Snapshot, entity: &Entity) -> String {
    format!(
        "{}  {:?}  {}  {}\n",
        entity.id,
        entity.kind,
        status(snapshot, entity),
        display::human_text(&entity.current.title).replace('\n', "\\n")
    )
}
fn status(snapshot: &Snapshot, entity: &Entity) -> &'static str {
    match entity.current.lifecycle {
        Lifecycle::Undecided => "未判断",
        Lifecycle::NotStarted
            if snapshot
                .check_operation(&entity.id, Operation::Start)
                .is_ok() =>
        {
            "着手可能"
        }
        Lifecycle::NotStarted => "依存待ち",
        Lifecycle::InProgress
            if entity.current.dependencies.iter().any(|id| {
                snapshot
                    .entity(id)
                    .is_ok_and(|e| e.current.lifecycle != Lifecycle::Completed)
            }) =>
        {
            "着手中・依存待ち"
        }
        Lifecycle::InProgress => "着手中",
        Lifecycle::Completed => "完了",
        Lifecycle::Cancelled => "取りやめ",
    }
}
fn sorted(mut entities: Vec<&Entity>) -> Vec<&Entity> {
    entities.sort_by(|a, b| (&a.created_at, &a.id).cmp(&(&b.created_at, &b.id)));
    entities
}
fn show(snapshot: &Snapshot, entity: &Entity) -> Result<String> {
    let mut out = row(snapshot, entity);
    let notes = snapshot.notes(&entity.id)?.len();
    if notes > 0 {
        out.push_str(&format!("{notes} notes\n"));
    }
    let parent = entity
        .current
        .parent
        .as_ref()
        .map(|id| snapshot.entity(id))
        .transpose()?;
    let parent_wait = entity.current.lifecycle == Lifecycle::NotStarted
        && parent.is_some_and(|e| e.current.lifecycle != Lifecycle::InProgress);
    if let Some(parent) = parent.filter(|_| !parent_wait) {
        out.push_str(&format!(
            "所属: {}  {}\n",
            parent.id,
            display::human_text(&parent.current.title)
        ));
    }
    let dependencies = entity
        .current
        .dependencies
        .iter()
        .map(|id| snapshot.entity(id))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if matches!(
        entity.current.lifecycle,
        Lifecycle::NotStarted | Lifecycle::InProgress
    ) {
        let unmet: Vec<_> = dependencies
            .into_iter()
            .filter(|e| e.current.lifecycle != Lifecycle::Completed)
            .collect();
        if parent_wait || !unmet.is_empty() {
            out.push_str(if entity.current.lifecycle == Lifecycle::NotStarted {
                "\n着手に必要\n"
            } else {
                "\n完了に必要\n"
            });
            if parent_wait {
                out.push_str(&format!("親の着手: {}", row(snapshot, parent.unwrap())));
            }
            for dependency in sorted(unmet) {
                out.push_str(&format!("依存先の完了: {}", row(snapshot, dependency)));
            }
        }
    }
    out.push('\n');
    out.push_str(&display::human_text(&entity.current.description));
    out.push('\n');
    if entity.kind == Kind::Group {
        let children = sorted(snapshot.children(&entity.id)?);
        let completed = children
            .iter()
            .filter(|e| e.current.lifecycle == Lifecycle::Completed)
            .count();
        let cancelled = children
            .iter()
            .filter(|e| e.current.lifecycle == Lifecycle::Cancelled)
            .count();
        out.push_str(&format!(
            "\n直属の子 {}/{}件終了（完了{completed}・取りやめ{cancelled}）\n",
            completed + cancelled,
            children.len()
        ));
        for child in children {
            out.push_str(&row(snapshot, child));
        }
        if snapshot
            .check_operation(&entity.id, Operation::Complete)
            .is_ok()
        {
            out.push_str("最終確認待ち\n");
        }
    }
    Ok(out)
}
fn actor(context: &Context) -> String {
    context
        .recorder
        .as_ref()
        .map(|r| display::human_text(&r.actor))
        .unwrap_or_else(|| "—".into())
}
fn recorder_display(context: &Context, details: bool) -> String {
    let mut text = actor(context);
    if details && let Some(recorder) = &context.recorder {
        text.push_str("  data: ");
        text.push_str(&display::human_text(
            serde_json::to_string(&recorder.data).expect("JSON object"),
        ));
    }
    text
}
struct Output {
    text: String,
    saved: bool,
}
fn output(text: String, saved: bool) -> Output {
    Output { text, saved }
}
fn branch_boundary<'a>(
    snapshot: &Snapshot,
    previous: &mut Option<&'a RecordId>,
    next: &'a RecordId,
    text: &mut String,
) -> Result<()> {
    if let Some(before) = previous
        && !snapshot.precedes(before, next)?
    {
        text.push_str("並行する分岐（直前の記録との先後関係なし）\n");
    }
    *previous = Some(next);
    Ok(())
}
fn run(command: Command) -> Result<Output> {
    let cwd = std::env::current_dir()?;
    if let Command::Init { prefix, backend } = command {
        let location = Location::discover(&cwd, true)?;
        location.init_backend(
            prefix.as_deref().unwrap_or("axon"),
            matches!(backend, Backend::File),
        )?;
        return Ok(output(
            format!(
                "Initialized {} at {}\n",
                if matches!(backend, Backend::File) {
                    "file"
                } else {
                    "SQLite"
                },
                display::human_text(
                    if matches!(backend, Backend::File) {
                        location.root.join(".axon/state.jsonl")
                    } else {
                        location.sqlite
                    }
                    .display()
                )
            ),
            true,
        ));
    }
    match command {
        Command::Storage {
            command: Storage::Check { snapshot },
        } => {
            axon::file::decode(&std::fs::read(snapshot)?)?;
            return Ok(output("Valid snapshot\n".into(), false));
        }
        Command::Merge { command } => {
            let saved = matches!(command, Merge::Apply { .. } | Merge::Driver { .. });
            match command {
                Merge::Prepare {
                    base,
                    ours,
                    theirs,
                    output,
                    workspace,
                } => axon::file_merge::prepare(
                    &base,
                    &ours,
                    &theirs,
                    &output,
                    &workspace,
                    context(),
                )?,
                Merge::Check { workspace } => axon::file_merge::check(&workspace)?,
                Merge::Apply { workspace } => axon::file_merge::apply(&workspace)?,
                Merge::Driver { base, ours, theirs } => {
                    axon::file_merge::driver(&base, &ours, &theirs, context())?
                }
            }
            return Ok(output("Merge operation succeeded\n".into(), saved));
        }
        _ => {}
    }
    let location = Location::discover(&cwd, false)?;
    let mut store = location.open()?;
    match command {
        Command::List
        | Command::Triage(_)
        | Command::Tasks(_)
        | Command::Show { .. }
        | Command::Log { .. }
        | Command::Note {
            command: Notes::List { .. },
        } => {
            let (_, snapshot) = store.read()?;
            let text = match command {
                Command::Triage(ref options) | Command::Tasks(ref options) => {
                    let kind = if matches!(command, Command::Triage(_)) {
                        CandidateList::Triage
                    } else {
                        CandidateList::Tasks
                    };
                    let evaluation = if options.trace_conditions {
                        condition::Evaluation::tracing(location.root, options.condition_timeout)
                    } else {
                        condition::Evaluation::with_timeout(
                            location.root,
                            options.condition_timeout,
                        )
                    };
                    candidates(&snapshot, kind, |entity, script| {
                        evaluation
                            .run_command(entity, script)
                            .map_err(|e| sqlite::Error::Invalid(e.to_string()))
                    })?
                    .into_iter()
                    .map(|e| row(&snapshot, e))
                    .collect()
                }
                Command::List => sorted(snapshot.entities().collect())
                    .into_iter()
                    .map(|e| row(&snapshot, e))
                    .collect(),
                Command::Show { id: value } => show(&snapshot, snapshot.entity(&id(value)?)?)?,
                Command::Log {
                    id: value,
                    recorder_details,
                } => {
                    let mut text = String::new();
                    let mut previous = None;
                    for record in snapshot.history(&id(value)?)? {
                        branch_boundary(&snapshot, &mut previous, &record.id, &mut text)?;
                        let description = match &record.event {
                            StateEvent::Created { initial, .. } => format!("登録: {initial:?}"),
                            StateEvent::Transition {
                                before,
                                after,
                                reason,
                                ..
                            } => format!(
                                "{before:?} → {after:?}{}",
                                reason
                                    .as_ref()
                                    .map(|r| format!("  理由: {}", display::human_text(r)))
                                    .unwrap_or_default()
                            ),
                            StateEvent::Integration {
                                inputs,
                                selected,
                                reason,
                            } => format!(
                                "統合: {:?} を採用{}",
                                inputs[*selected].current.lifecycle,
                                reason
                                    .as_ref()
                                    .map(|r| format!("  理由: {}", display::human_text(r)))
                                    .unwrap_or_default()
                            ),
                        };
                        text.push_str(&format!(
                            "{}  {}  {description}\n",
                            display::timestamp(&record.context.at),
                            recorder_display(&record.context, recorder_details)
                        ));
                    }
                    text
                }
                Command::Note {
                    command:
                        Notes::List {
                            id: value,
                            recorder_details,
                        },
                } => {
                    let mut text = String::new();
                    let mut previous = None;
                    for note in snapshot.notes(&id(value)?)? {
                        branch_boundary(&snapshot, &mut previous, &note.id, &mut text)?;
                        text.push_str(&format!(
                            "{}  {}  {}\n{}\n\n",
                            note.id,
                            display::timestamp(&note.context.at),
                            recorder_display(&note.context, recorder_details),
                            display::human_text(&note.body)
                        ));
                    }
                    text
                }
                _ => unreachable!(),
            };
            Ok(output(text, false))
        }
        Command::Capture(args) => create(&mut store, Kind::Issue, Lifecycle::Undecided, args),
        Command::Plan(args) => create(&mut store, Kind::Issue, Lifecycle::NotStarted, args),
        Command::Group {
            command: Group::Capture(args),
        } => create(&mut store, Kind::Group, Lifecycle::Undecided, args),
        Command::Group {
            command: Group::Plan(args),
        } => create(&mut store, Kind::Group, Lifecycle::NotStarted, args),
        Command::Write {
            id: value,
            title,
            body,
        } => {
            let id = id(value)?;
            let body = body.read()?;
            if title.is_none() && body.is_none() {
                return Err(sqlite::Error::Invalid(
                    "write requires --title, --description or --description-file".into(),
                ));
            }
            store.update(|_, snapshot| {
                snapshot.write(&id, title, body)?;
                Ok(())
            })?;
            Ok(output(format!("{id}  Updated\n"), true))
        }
        Command::Note {
            command:
                Notes::Add {
                    id: value,
                    message,
                    file,
                },
        } => {
            let id = id(value)?;
            let body = Body {
                description: message,
                description_file: file,
            }
            .read()?
            .unwrap_or_default();
            let note =
                store.update(|_, snapshot| Ok(snapshot.add_note(&id, body, context())?))?;
            Ok(output(format!("{id}  Note {note}\n"), true))
        }
        Command::Group { command } => {
            let (value, parent) = match command {
                Group::Set { id, parent } => (id, Some(crate::id(parent)?)),
                Group::Unset { id } => (id, None),
                _ => unreachable!(),
            };
            let id = id(value)?;
            store.update(|_, snapshot| {
                snapshot.set_parent(&id, parent)?;
                Ok(())
            })?;
            Ok(output(format!("{id}  Parent updated\n"), true))
        }
        Command::When { command } => {
            let (value, command) = match command {
                When::Set { id, command } => (id, Some(command)),
                When::Clear { id } => (id, None),
            };
            let id = id(value)?;
            store.update(|_, snapshot| {
                snapshot.set_condition(&id, command)?;
                Ok(())
            })?;
            Ok(output(format!("{id}  Condition updated\n"), true))
        }
        Command::Dep { command } => {
            let (value, needs, add) = match command {
                Dependency::Add { id, needs } => (id, needs, true),
                Dependency::Rm { id, needs } => (id, needs, false),
            };
            let id = id(value)?;
            let needs = crate::id(needs)?;
            store.update(|_, snapshot| {
                if add {
                    snapshot.add_dependency(&id, &needs)?;
                } else {
                    snapshot.remove_dependency(&id, &needs)?;
                }
                Ok(())
            })?;
            Ok(output(format!("{id}  Dependency updated\n"), true))
        }
        command => {
            let (args, operation) = match command {
                Command::Accept(args) => (args, Operation::Accept),
                Command::Withdraw(args) => (args, Operation::Withdraw),
                Command::Start(args) => (args, Operation::Start),
                Command::Release(args) => (args, Operation::Release),
                Command::Done(args) => (args, Operation::Complete),
                Command::Cancel(args) => (args, Operation::Cancel),
                Command::Reconsider(args) => (args, Operation::Reconsider),
                _ => unreachable!(),
            };
            let id = id(args.id)?;
            let state = store.update(|_, snapshot| {
                snapshot.perform(&id, operation, args.reason, context())?;
                Ok(snapshot.entity(&id)?.current.lifecycle)
            })?;
            Ok(output(format!("{id}  {state:?}\n"), true))
        }
    }
}
fn create(
    store: &mut axon::location::Store,
    kind: Kind,
    lifecycle: Lifecycle,
    args: Create,
) -> Result<Output> {
    let description = args.body.read()?.unwrap_or_default();
    let parent = args.parent.map(id).transpose()?;
    let dependencies = args
        .needs
        .into_iter()
        .map(id)
        .collect::<Result<BTreeSet<_>>>()?;
    let id = store.update(|prefix, snapshot| {
        let id = id(format!("{prefix}-{:032x}", rand::random::<u128>()))?;
        snapshot.create(
            id.clone(),
            kind,
            Current {
                title: args.title,
                description,
                lifecycle,
                condition: None,
                parent,
                dependencies,
            },
            context(),
        )?;
        Ok(id)
    })?;
    Ok(output(format!("{id}  {kind:?}  {lifecycle:?}\n"), true))
}
fn main() -> std::process::ExitCode {
    let result = run(Cli::parse().command);
    match result {
        Ok(output) => {
            let mut stdout = std::io::stdout().lock();
            match stdout
                .write_all(output.text.as_bytes())
                .and_then(|_| stdout.flush())
            {
                Ok(()) => std::process::ExitCode::SUCCESS,
                Err(error) => {
                    let _ = writeln!(
                        std::io::stderr(),
                        "{}: {error}",
                        if output.saved {
                            "storage applied; output failed; inspect saved state before retrying"
                        } else {
                            "output failed"
                        }
                    );
                    std::process::ExitCode::from(1)
                }
            }
        }
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "{}", display::human_text(error));
            std::process::ExitCode::from(1)
        }
    }
}
