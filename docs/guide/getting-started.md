# 使い始める

AxonはIssueとGroupを一つのlifecycleで管理します。以下はHomebrewで導入し、独立した保存先で登録から完了までを試す手順です。

## インストールと対応環境

配布バイナリはmacOS 15（Sequoia）以上のApple Silicon / Intelに対応します。[Homebrew](https://brew.sh)で導入でき、Rustは不要です。LinuxとWSL2はソースビルドでの未検証のbest effort、native Windowsは非対応です。

```sh
brew install gin0606/tap/axon
```

更新には次を使います。

```sh
brew update
brew upgrade gin0606/tap/axon
```

### 開発者向けのソースビルド

最低Rust versionはrootの `Cargo.toml` の `rust-version`、開発toolchainは [検証方針](../development/verification.md) を参照してください。

```sh
git clone https://github.com/gin0606/axon.git
cd axon
cargo install --locked --path .
```

Cargoの実行ファイルの保存先（通常は `~/.cargo/bin`）を `PATH` に追加してください。checkout内だけで試す場合は `cargo build --locked --bin axon` でビルドし、以下の `AXON_BIN` に `target/debug/axon` の絶対パスを指定します。

## 独立した保存先で試す

Homebrewで導入した実行ファイルを固定し、一時ディレクトリで試します。

```sh
AXON_BIN="$(brew --prefix gin0606/tap/axon)/bin/axon"
"$AXON_BIN" --version
"$AXON_BIN" --help
AXON_TRIAL_DIR="$(mktemp -d)"
cd "$AXON_TRIAL_DIR"
"$AXON_BIN" init trial
"$AXON_BIN" capture --accept --label feat --title '最初の仕事' -m '目的と完了条件'
```

登録には仕事の種類を表す `--label` が必須です。値と意味は [日常の操作](usage.md#labelで仕事の種類を示す) にあります。以降の `ID` は直前の登録出力で返ったIDへ置き換えます。試用先はGit repository外で、既存の `.axon` を持つディレクトリの配下も避けてください。

```sh
"$AXON_BIN" tasks
"$AXON_BIN" show ID
"$AXON_BIN" start ID
"$AXON_BIN" note add ID -m '確認した結果'
"$AXON_BIN" complete ID
"$AXON_BIN" log ID --recorder-details
"$AXON_BIN" note list ID --recorder-details
```

初期化は新規作成専用です。`axon init` は管理rootに記録のdirectory `.axon/records/`、header `.axon/header.json`、lockと一時fileだけを除外する `.axon/.gitignore`、Gitの改行変換を止める `* -text` の1行だけの `.axon/.gitattributes` を作ります。repository rootのfileとGit configには触れません。保存形式は一つ、配置や形式を選ぶoptionはありません。

## Git repositoryで使う

Gitが保存先を無視するか追跡するかにAxonは関与しません。Git内での使い方は二つあり、どちらになるかは利用者のGitの運用だけで決まります。

```sh
AXON_GIT_TRIAL_DIR="$(mktemp -d)"
cd "$AXON_GIT_TRIAL_DIR"
git init
"$AXON_BIN" init trial
"$AXON_BIN" capture --accept --label feat --title 'Gitのrepositoryで管理する仕事' -m '目的と完了条件'
```

作った直後のheader、`.axon/.gitignore`、`.axon/.gitattributes`（記録を作ればその記録も）はGitからuntrackedに見え、`git add -A` すればcommitされます。二つの運用のどちらを使うかを決め、一つのrepositoryでは混ぜないでください。

無視する運用は、`.git/info/exclude` やglobalのignore fileに `.axon/` の行を書いて、利用者がGitに無視させて選びます。linked worktreeには `.axon` が現れないため、全worktreeがmain worktreeの保存先を共有します。worktreeごとに `axon init` を繰り返す必要はありません。無視した保存先はGitのcheckout・mergeの上書きから保護されなくなり、`.axon/` を追跡しているcommitを取り込むと警告なしに置き換わります。

追跡する運用は、`git add .axon` で記録、header、`.axon/.gitignore`、`.axon/.gitattributes` をstageしてcommitして選びます。`axon init` が作った `.axon/.gitignore` がlockと一時fileを除き、`.axon/.gitattributes` が記録fileをcheckout時の改行変換から外すので、ほかにGitの設定は要りません（属性の適用範囲と改行変換が起きた場合の対処は [保存先とworktree](storage.md#改行変換と-axongitattributes)）。各worktreeが自分の保存先を持ち、branchごとに分岐した計画と記録をGitで取り込めます。両側が記録を追加したbranchは、記録が別fileなのでmergeの属性やGitの設定なしでそのままmergeできます（契約の範囲はローカルのGit操作で、GitHub上のmergeも確認済み。[保存先とworktree](storage.md)）。統合後は `axon storage check` で衝突・違反・記録の欠けを確認し、`axon resolve` と通常操作で直します。Axonの状態の取り消しはlifecycle操作（`axon reopen` など）で行い、Gitのrevertに頼らないでください。`axon init` は `.axon/` の外については手順を表示するだけで、repository rootの `.gitignore`、`.gitattributes`、Git configを作成も編集もしません。

通常操作はどちらの運用でも同じです。統合の検査と解決は [file保存とGit統合](../development/lifecycle-file.md#git-統合と検査)、保存先の選ばれ方は [保存先とworktree](storage.md) を参照してください。

## Agent向けskill

[`axon-kit`](../../plugins/axon-kit/skills) は操作契約、[`axon`](../../plugins/axon/skills) は任意の個人用協業方針です。plugin内に必要なreferenceを同梱しているため、利用先repositoryにAxonのソースcheckoutを置く必要はありません。対応するCLIとpluginを対象環境へ導入し、管理するrepositoryで呼び出します。

binaryを指定した場合はその指定を、未指定なら対象環境で発見した `axon` を使います。skillは`axon --version`・`axon --help`で対応を照合して実行ファイルとrootを固定します。対象環境のCLIとsessionに読み込まれたskillのversionが一致しない場合は、その不一致を解決してから操作します。開発版の試用では上記の絶対パスを渡す方法も使えます。

同梱skillは、Groupを直接着手せず配下から実効lifecycleを導出する規則（[Group の実効 lifecycle](../reference/lifecycle.md#group-の実効-lifecycle)）、`axon reopen`、記録の集合の保存先と `axon storage check`・`axon resolve`・`axon convert`、必須のlabel（`axon capture --label`・`axon label set`）と `axon-declaration/v2` を前提にします。

例えば「このrepositoryの懸念をAxonに未判断として記録して」と依頼できます。対象と任せる範囲は依頼が決め、登録から実装・commitの権限を推測しません。
