# 使い始める

単一 lifecycle の新 CLI を、まず独立した SQLite 保存先で試します。既存の PATH 上の `axon` と、この checkout でビルドした binary は別物として扱ってください。

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
"$AXON_BIN" plan --title '最初の仕事' -m '目的と完了条件'
```

以降の `ID` は直前の登録出力で返ったIDへ置き換えます。試用先はGit repository外で、既存の `.axon` を持つディレクトリの配下も避けてください。

```sh
"$AXON_BIN" tasks
"$AXON_BIN" show ID
"$AXON_BIN" start ID
"$AXON_BIN" note add ID -m '確認した結果'
"$AXON_BIN" done ID
"$AXON_BIN" log ID --recorder-details
"$AXON_BIN" note list ID --recorder-details
```

初期化は新規作成専用です。SQLiteの置き場所とGit worktree共有は [保存先](storage.md) を参照してください。file backendとmergeの利用手順は [file保存とGit統合](../development/lifecycle-file.md) を参照してください。

## Agent向けskill

この checkout の [`plugins/axon-kit/skills`](../../plugins/axon-kit/skills) が新lifecycleの操作契約、[`plugins/axon/skills`](../../plugins/axon/skills) が任意の個人用協業方針です。利用中のagentに対応するローカルplugin読み込み方法でこのcheckoutを指定するか、必要なSKILL.mdを直接読ませ、選択したbinaryの絶対パスと保存先を一緒に渡してください。配布済みpluginや既存sessionに読み込まれたskillは旧版の可能性があります。自動で再インストール・切替はしません。

例えば「このcheckoutのaxon-kit conventionsとcaptureを読み、指定binaryでこの懸念を未判断として残して」と依頼できます。操作可能かは新binaryのhelpと照合します。判断する対象と任せる範囲は依頼が決めます。

## 旧データを持ち込む場合

旧schemaの自動移行・一括取り込みはありません。旧binaryと旧保存先を維持したまま読み取り、新しい空の保存先へ必要な計画を手動で登録します。目的、本文、関係、今後の扱いを確認し、未判断はcapture、採用済みはplanを使います。旧状態を機械的に対応付けたり、過去の日時・記録者・IDを再現した履歴として作ったりしません。必要な旧記録は出典を示したNoteとして残せます。依存・所属は新IDで照合してください。

既存の `.axon` を上書き・コピーして新形式とみなさず、メインrepositoryのbinary・実データ・管理計画の切替は別途明示された作業として扱います。
