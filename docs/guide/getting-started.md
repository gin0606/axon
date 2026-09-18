# 使い始める

Axonは単一lifecycleでIssueとGroupを管理します。対応するbinaryをPATHから使う場合も、開発版を絶対パスで使う場合も、`axon --version` と `axon --help` で選んだ版を確認します。以下は独立した保存先で試す手順です。

## インストールと対応環境

macOSをサポート対象とし、LinuxとWSL2は未検証のbest effort、native Windowsは非対応です。最低Rust versionは1.89、開発toolchainは [検証方針](../development/verification.md) を参照してください。

この checkout でビルドし、絶対パスを一度選びます。`cargo install` や既存 binary の置換は不要です。

```sh
cargo build --locked --bin axon
AXON_BIN="$(pwd)/target/debug/axon"
"$AXON_BIN" --help
AXON_TRIAL_DIR="$(mktemp -d)"
cd "$AXON_TRIAL_DIR"
"$AXON_BIN" init trial
"$AXON_BIN" capture --accept --title '最初の仕事' -m '目的と完了条件'
```

以降の `ID` は直前の登録出力で返ったIDへ置き換えます。試用先はGit repository外で、既存の `.axon` を持つディレクトリの配下も避けてください。

```sh
"$AXON_BIN" tasks
"$AXON_BIN" show ID
"$AXON_BIN" start ID
"$AXON_BIN" note add ID -m '確認した結果'
"$AXON_BIN" complete ID
"$AXON_BIN" log ID --recorder-details
"$AXON_BIN" note list ID --recorder-details
```

初期化は新規作成専用です。SQLiteの置き場所とGit worktree共有は [保存先](storage.md) を参照してください。file backendと`axon merge`の利用手順は [file保存とGit統合](../development/lifecycle-file.md) を参照してください。

## 保存方式を選ぶ

| 方式 | 用途と共有の単位 |
| --- | --- |
| SQLite（既定） | 同じrepositoryのworktreeで一つの保存先を共有する |
| file | worktreeごとに分岐して、Gitで計画と記録を取り込む |

fileを試す場合は、上のSQLite試用先と別の空directoryで初期化します。

```sh
AXON_FILE_TRIAL_DIR="$(mktemp -d)"
cd "$AXON_FILE_TRIAL_DIR"
git init
"$AXON_BIN" init trial --backend file
"$AXON_BIN" capture --accept --title 'Gitで共有する仕事' -m '目的と完了条件'
```

通常操作は両方式で同じです。fileのGit driver登録・追跡・競合解決は [file保存とGit統合](../development/lifecycle-file.md#git-driver) に従います。backendの変更に`axon init`を使わず、新規保存先を別に選びます。

## Agent向けskill

[`axon-kit`](../../plugins/axon-kit/skills) は操作契約、[`axon`](../../plugins/axon/skills) は任意の個人用協業方針です。plugin内に必要なreferenceを同梱しているため、利用先repositoryにAxonのソースcheckoutを置く必要はありません。対応するCLIとpluginを対象環境へ導入し、管理するrepositoryで呼び出します。

binaryを指定した場合はその指定を、未指定なら対象環境で発見した `axon` を使います。skillは`axon --version`・`axon --help`で対応を照合して実行ファイルとrootを固定します。対象環境のCLIとsessionに読み込まれたskillのversionが一致しない場合は、その不一致を解決してから操作します。開発版の試用では上記の絶対パスを渡す方法も使えます。

例えば「このrepositoryの懸念をAxonに未判断として記録して」と依頼できます。対象と任せる範囲は依頼が決め、登録から実装・commitの権限を推測しません。
