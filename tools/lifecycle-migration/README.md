# 単一lifecycleへの専用移行ツール

［公開用編集：外部プロジェクトの識別情報・運用詳細を一般化しています。］


旧file format 1/schema 13・14、旧SQLite schema 13・14を現行の同じbackendへ移す、一度の移行用ツールです。通常の `axon` には組み込みません。[設計と対象の合意](../../docs/development/lifecycle-migration-tool.md) に基づき、明示したrootだけを扱います。

## ビルドと準備

このrepositoryのcheckoutからビルドします。独立workspaceとlockfileを持つため、rootの通常のbuild/testでは実行されません。

```sh
cargo build --locked --manifest-path tools/lifecycle-migration/Cargo.toml
cargo test --locked --manifest-path tools/lifecycle-migration/Cargo.toml
cargo clippy --locked --manifest-path tools/lifecycle-migration/Cargo.toml --all-targets -- -D warnings
```

実行ファイルは `tools/lifecycle-migration/target/debug/axon-lifecycle-migration` です。正式なjobを作る前に、移行ツール・新 `axon`・必要なら旧 `axon` の実体を管理root外の固定directoryへコピーしてください。jobはそれらの絶対pathとBLAKE3 digestを固定し、以後の置換を拒否します。`cargo build` の出力や共有binaryを直接指定すると、再build・更新で復旧も止まるため、jobが不要になるまで固定コピーを保持します。

全pathにsymlinkを解決した絶対pathを指定します。たとえばmacOSの `/tmp` は `/private/tmp` です。jobは全対象rootの外側に置く専用directoryです。sourceはfileならそのworktreeの `.axon/state.jsonl`、SQLiteならGit common directory側の `.axon/axon.db` を指定します。共有SQLiteの複数worktreeを重複登録できません。

configの例:

```json
{
  "version": 1,
  "axon_binary": "/absolute/frozen-bin/axon",
  "legacy_binary": "/absolute/frozen-bin/axon-old",
  "targets": [
    {
      "name": "project",
      "root": "/absolute/project",
      "source": "/absolute/project/.axon/axon.db",
      "backend": "sqlite",
      "input": "legacy"
    },
    {
      "name": "axon",
      "root": "/absolute/axon-worktree",
      "source": "/absolute/axon-worktree/.axon/state.jsonl",
      "backend": "file",
      "input": "current"
    }
  ]
}
```

`backend` は `file` / `sqlite`、`input` は `legacy` / `current`。`current` は既存の移行成果を全recordごと保持し、置換しません。形式だけから既存移行を推測する入口ではなく、指定したsourceと固定候補の一致を検証します。`legacy_binary` は省略できます。指定binaryや保存済みshell条件をprepare中に実行しません。

今回の対象はdotfiles、external-project-a-docs、external-project-a、axon、cacheexec、ediro、external-project-b、sutologです。Axon自身はこのworktreeの移行後の追加記録を含む現行snapshotを採用します。旧mainを別storeへ再変換して合流させる用途には使いません。正式採用時はGitの祖先関係・両側の追加変更を再確認して既存移行を反映し、その後のmainを `current` として新しいjobに指定します。

## prepare

```sh
/absolute/frozen-bin/axon-lifecycle-migration prepare \
  --config /absolute/config.json --job /absolute/jobs/adoption
```

backup・変換・全field/record/関係の照合を行い、正本は置換しません。fileは旧新版のlockを取得します。SQLiteはbackup APIでWALの確定データを含めたsnapshotを取得します。SQLiteの通常の読取に伴うSHMやlock fileは作成される場合があります。

全targetが成功した場合だけ `checked.json` を作ります。同じconfigとbinaryでprepareを繰り返すと固定済みsource、移行時刻、IDを再利用します。sourceやjob内部の変更は拒否します。失敗時にもbackupと途中artifactは削除しません。sourceが変わった場合は元jobを保持して別jobを作ります。同じ旧storeのbranchを複数targetとして変換することも拒否します。

| artifact | 内容 |
| --- | --- |
| `plan.json` | config、固定移行ID・時刻、binaryのpathとdigest |
| `TARGET/source` | 元file bytes、または整合SQLite backup |
| `TARGET/source-proof.json` | 変換前に固定するsourceの物理・論理digest |
| `TARGET/candidate` | 同backendの候補 |
| `TARGET/candidate.jsonl` | 現行file codecで再読照合した全snapshot |
| `TARGET/report.json` | 全Entityの状態・条件・補正理由と移植元Note ID |
| `TARGET/prepared.json`、`checked.json` | artifactのdigestとGit topology/indexの固定値 |
| `TARGET/journal.json` | 適用・復旧のintentと検証済み完了の履歴 |
| `TARGET/apply-*`、`restore-*` | 切替直前の整合preimageとSQLiteの物理file set |

各Entityの「Legacy source records」Noteに、元Header/metadataと所有する全旧rowを保存します。SQLiteは各列の型と値を残し、REALはIEEE 754のbits、BLOBはbyte列です。既存Noteは元ID・本文・記録者・時刻・因果関係も独立したNoteとして保持します。旧claimのworktreeやRevisionも原文に残り、新logは過去の操作の再現ではなく移行時の状態設定と明記します。

保存済みCommandは、旧CLI・path・schemaへの依存を実行せずに確認し、対象の `rules.reviewed_commands` にIDと元文字列を指定します。文字列が変わると拒否します。日時と未成立AfterEntityの生成条件には、移行後の環境で `python3` が必要です。AfterEntityにはPATH上の新 `axon` も必要です。移行ツール自体への恒久依存はありません。評価失敗はexit 2、未成立はexit 1です。

一意に対応できない場合は、`TARGET/source-proof.json` の `logical_digest` を参照して新configと新jobを作ります。overrideの例:

```json
{
  "rules": {
    "source_digest": "source-proof.jsonのlogical_digest",
    "reviewed_commands": { "project-abcdef": "元のCommand全文" },
    "overrides": {
      "project-ghijkl": {
        "reason": "元の意味と、この対応を採用した具体的な理由",
        "lifecycle": "Cancelled",
        "condition": "Always"
      }
    }
  }
}
```

`lifecycle` または `condition` を `null` にすると、その項目の基本マッピングを使います。条件の置換は `{"Command":"shell文字列"}`、解除は `"Always"`。理由なし、元digest不一致、存在しない対象、Cancelled祖先に矛盾するoverrideは拒否します。未知schema・field・型・参照切れをoverrideで無視することはできません。

## applyとrestore

候補とreportを確認し、全writer、SQLite接続、editor、Git操作を停止した切替窓で実行します。`--writers-stopped` はこの運用確認であり、保存claimやOS lockだけで停止を証明したものではありません。

```sh
/absolute/frozen-bin/axon-lifecycle-migration apply \
  --job /absolute/jobs/adoption --writers-stopped
```

fileは旧 `write.lock` と新 `state.lock` を保持し、source、backend、Git indexを再照合してから同期済みtemporaryをrenameします。SQLiteは整合preimageと停止したDB/WAL/SHMを保全し、TRUNCATE checkpointとDELETE journalへの切替が成功し、sidecarが残っていない場合に同期済み候補へrenameします。これにより旧WALを新DBの横に残しません。SQLiteの停止処理の根拠は [wal_checkpoint](https://sqlite.org/pragma.html#pragma_wal_checkpoint) と [WALの終了時処理](https://sqlite.org/wal.html#the_wal_file) を参照してください。

rootごとに処理し、失敗したら後続rootは処理しません。終了コード1では部分適用の可能性があるため、jobを保持して `journal.json` と正本を確認します。intentの後に停止した場合、同じ操作を同じjobで再実行すると、正本が元snapshotか候補かを照合して再開します。両方と違えば停止します。全rootを一つのatomic transactionにはしません。

復旧も同じ停止確認が必要です。

```sh
/absolute/frozen-bin/axon-lifecycle-migration restore \
  --job /absolute/jobs/adoption --writers-stopped
```

現在の正本を追加保全してから対応するbackupへ戻します。移行後の追記・変更があれば自動復元を拒否するので、その新情報を引き継ぐ判断が必要です。fileは元bytesへ、SQLiteはWAL上の確定情報を含めた元論理snapshotへ戻します。復旧に入ったjobの再applyは拒否し、改めてprepareする必要があります。

Gitのmerge/stage/commit、共有binary/PATH、plugin、Git driverの切替はツールの外で行います。全対象と利用binaryを合わせるまで通常利用を再開しません。backupとjobの自動削除はありません。

成功時はstdoutにtargetごとのJSON結果、失敗時はstderrに英語の理由を出します。終了コードは成功0、実行失敗1、引数不正2です。

実施時点と入力を限定した検証結果は [2026-09-13の隔離検証](../../docs/development/audits/20260913-lifecycle-migration-tool.md) に記録しています。
