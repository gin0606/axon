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

初期化は新規作成専用です。`axon init` は管理rootに記録のdirectory `.axon/records/`、header `.axon/header.json`、lockと一時fileだけを除外する `.axon/.gitignore` を作ります。repository rootのfileとGit configには触れません。保存形式は一つ、配置や形式を選ぶoptionはありません。

## Git repositoryで使う

Gitが保存先をどう扱うかにAxonは関与しません。Git内での使い方は二つあり、どちらになるかは利用者のGitの運用だけで決まります。

```sh
AXON_GIT_TRIAL_DIR="$(mktemp -d)"
cd "$AXON_GIT_TRIAL_DIR"
git init
"$AXON_BIN" init trial
"$AXON_BIN" capture --accept --title 'Gitのrepositoryで管理する仕事' -m '目的と完了条件'
```

作った直後のheaderと `.axon/.gitignore`（記録を作ればその記録も）はGitからuntrackedに見え、`git add -A` すればcommitされます。二つの運用のどちらを使うかを決め、一つのrepositoryでは混ぜないでください。

無視する運用は、`.git/info/exclude` やglobalのignore fileに `.axon/` の行を書いて、利用者がGitに無視させて選びます。linked worktreeには `.axon` が現れないため、全worktreeがmain worktreeの保存先を共有します。worktreeごとに `axon init` を繰り返す必要はありません。無視した保存先はGitのcheckout・mergeの上書きから保護されなくなり、`.axon/` を追跡しているcommitを取り込むと警告なしに置き換わります。

追跡する運用は、`git add .axon` で記録とheaderをstageしてcommitして選びます。`axon init` が作った `.axon/.gitignore` がlockと一時fileを除くので、ほかにGitの設定は要りません。各worktreeが自分の保存先を持ち、branchごとに分岐した計画と記録をGitで取り込めます。両側が記録を追加したbranchは、記録が別fileなのでGitの属性や設定なしでそのままmergeできます（契約の範囲はローカルのGit操作で、GitHub上のmergeも確認済み。[保存先とworktree](storage.md)）。統合後は `axon storage check` で衝突・違反・記録の欠けを確認し、`axon resolve` と通常操作で直します。Axonの状態の取り消しはlifecycle操作（`axon reopen` など）で行い、Gitのrevertに頼らないでください。`axon init` は手順を表示するだけで、repositoryの `.gitignore`、`.gitattributes`、Git configを作成も編集もしません。

通常操作はどちらの運用でも同じです。統合の検査と解決は [file保存とGit統合](../development/lifecycle-file.md#git-統合と検査)、保存先の選ばれ方は [保存先とworktree](storage.md) を参照してください。

## Agent向けskill

[`axon-kit`](../../plugins/axon-kit/skills) は操作契約、[`axon`](../../plugins/axon/skills) は任意の個人用協業方針です。plugin内に必要なreferenceを同梱しているため、利用先repositoryにAxonのソースcheckoutを置く必要はありません。対応するCLIとpluginを対象環境へ導入し、管理するrepositoryで呼び出します。

binaryを指定した場合はその指定を、未指定なら対象環境で発見した `axon` を使います。skillは`axon --version`・`axon --help`で対応を照合して実行ファイルとrootを固定します。対象環境のCLIとsessionに読み込まれたskillのversionが一致しない場合は、その不一致を解決してから操作します。開発版の試用では上記の絶対パスを渡す方法も使えます。

同梱skillは、Groupを直接着手せず配下から実効lifecycleを導出する規則（[Group の実効 lifecycle](../reference/lifecycle.md#group-の実効-lifecycle)）と、`axon reopen` を前提にします。

例えば「このrepositoryの懸念をAxonに未判断として記録して」と依頼できます。対象と任せる範囲は依頼が決め、登録から実装・commitの権限を推測しません。
