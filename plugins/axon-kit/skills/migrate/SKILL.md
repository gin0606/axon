---
name: migrate
description: 保存済みの Axon current-schema SQLite snapshot を、live root を切り替えず検証済みの SQLite または file-backend 出力へ変換する。axon migrate とその artifact の復旧に使い、通常の schema open、root 初期化、merge conflict には使わない。
---

# 保存済み Axon snapshot を migrate する

`axon-kit:conventions` を使い、mutation contract を読む。Migration は新しい artifact set を作成するが、source の変更や active root の切り替えは行わない。

## migration 境界を固定する

呼び出し側が、正確な source SQLite database、未使用の output directory、target backend の `sqlite` または `file` を与える。source は current schema と一致しなければならない。この command は schema update を実行しない。support されている自動 schema update は通常の command が担う。retired format には互換 build または別途計画した one-time conversion が必要である。

最終変換の前に source へのすべての writer を停止し、source database、すべての WAL/SHM file、調査に必要な正確な旧・新 binary を保存する。それらの path と digest を記録する。source の固定中に Note や他の bookkeeping を追加しない。

target backend の選択は user または呼び出し側 workflow の判断である。File storage は worktree-local で Git-trackable、Git SQLite は common Git directory の parent 配下の `.axon/axon.db` で共有される。この capability はその tradeoff を選択せず、current root から推測もしない。

## 1 回だけ変換する

`axon migrate --source <db> --output <unused-directory> --backend <sqlite|file>` を単独の mutation として実行する。失敗または不確かな試行の後、その output directory を削除・再利用してはならない。

成功時は最終 manifest と記録されたすべての digest を検証する。想定する保存済み source backup、target の `axon.db` または `state.jsonl`、canonical な `snapshot.jsonl` が揃い、検証が完了していることを確認する。独立した read-only check として `axon storage check <output>/snapshot.jsonl` を実行する。

実用的な場合は、検証済み target store を別の disposable root に copy し、candidate binary で Entity、Note、Revision、history を調査する。この検証用 copy を live result として使わない。

backend 設定は生成されない。Migration は Git ignore/attribute rule の作成や driver 登録を行わず、それらは別の cutover task である。Init は新規作成専用で、cutover 後の integration を repair できない。

## cutover を分離する

Migration 出力は active-root の切り替えではない。呼び出し側が cutover を別途許可し所有しない限り、storage の置き換え、tracked file の更新、writer の再起動、旧 version の削除を行わない。cutover では writer を停止したままにし、旧 root と設定を保存し、保持 data を上書きせず選択した backend の canonical state だけを配置し、意図するすべての worktree を 1 つの互換 binary と store identity に対して検証する。

top-level manifest が存在しない、読み取れない、不完全、または digest mismatch の場合、結果を採用しない。出力全体を保存し、その diagnostic boundary から試行を分類する。新しい試行には別の未使用 directory と同じ検証済み source snapshot を使う。path を再利用するためだけに曖昧な試行を消去しない。

## 結果を返す

source と binary の identity、target backend、output path、manifest と検証結果、保持すべき backup、storage 結果の分類、live cutover が未完了かを返す。
