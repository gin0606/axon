# 使い始める

AxonはIssueとGroupを一つのlifecycleで管理します。対応するbinaryをPATHから使う場合も、開発版を絶対パスで使う場合も、`axon --version` と `axon --help` で選んだ版を確認します。以下は独立した保存先で試す手順です。

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

初期化は新規作成専用です。`axon init` は管理rootに正本 `.axon/state.jsonl` を作ります。Gitに関するfileは作りません。保存形式は一つ、配置や形式を選ぶoptionはありません。

## Git repositoryで使う

Gitが保存先をどう扱うかにAxonは関与しません。Git内での使い方は二つあり、どちらになるかは利用者のGitの運用だけで決まります。

```sh
AXON_GIT_TRIAL_DIR="$(mktemp -d)"
cd "$AXON_GIT_TRIAL_DIR"
git init
"$AXON_BIN" init trial
"$AXON_BIN" capture --accept --title 'Gitのrepositoryで管理する仕事' -m '目的と完了条件'
```

作った直後の正本はGitからuntrackedに見え、`git add -A` すればcommitされます。二つの運用のどちらを使うかを決め、一つのrepositoryでは混ぜないでください。

無視する運用は、`.git/info/exclude` やglobalのignore fileに `.axon/` の行を書いて、利用者がGitに無視させて選びます。linked worktreeには `.axon` が現れないため、全worktreeがmain worktreeの保存先を共有します。worktreeごとに `axon init` を繰り返す必要はありません。無視した正本はGitのcheckout・mergeの上書きから保護されなくなり、正本を追跡しているcommitを取り込むと警告なしに置き換わります。

追跡する運用は、`axon init` が表示する手順を実行して選びます。正本だけを追跡対象にする `.axon/.gitignore` を作り、`.gitattributes` にmerge driverを宣言し、driverを登録して、stage・commitします。各worktreeが自分の正本を持ち、branchごとに分岐した計画と記録をGitで取り込めます。`axon init` は手順を表示するだけで、`.gitignore`、`.gitattributes`、Git configを作成も編集もしません。

通常操作はどちらの運用でも同じです。driverの登録・追跡・競合解決は [file保存とGit統合](../development/lifecycle-file.md#git-driver)、保存先の選ばれ方は [保存先とworktree](storage.md) を参照してください。

## Agent向けskill

[`axon-kit`](../../plugins/axon-kit/skills) は操作契約、[`axon`](../../plugins/axon/skills) は任意の個人用協業方針です。plugin内に必要なreferenceを同梱しているため、利用先repositoryにAxonのソースcheckoutを置く必要はありません。対応するCLIとpluginを対象環境へ導入し、管理するrepositoryで呼び出します。

binaryを指定した場合はその指定を、未指定なら対象環境で発見した `axon` を使います。skillは`axon --version`・`axon --help`で対応を照合して実行ファイルとrootを固定します。対象環境のCLIとsessionに読み込まれたskillのversionが一致しない場合は、その不一致を解決してから操作します。開発版の試用では上記の絶対パスを渡す方法も使えます。

同梱skillは、Groupを直接着手せず配下から実効lifecycleを導出する規則（[Group の実効 lifecycle](../reference/lifecycle.md#group-の実効-lifecycle)）と、`axon reopen` を前提にします。

例えば「このrepositoryの懸念をAxonに未判断として記録して」と依頼できます。対象と任せる範囲は依頼が決め、登録から実装・commitの権限を推測しません。
