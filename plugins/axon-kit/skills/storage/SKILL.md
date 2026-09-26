---
name: storage
description: Axonの保存先を`axon init`で新規初期化し、求められれば運用の手順を案内し、保存先を`axon storage check`で検査する。衝突・違反の解決や既存データの修復には使わない。
---

# 新規初期化と保存先の検査

`axon-kit:conventions` を使い、共通契約で選択したbinary・対象rootを維持する。`axon init [prefix]` は管理rootの `.axon/` に記録のdirectory `.axon/records/`、header `.axon/header.json`、lockと一時fileだけを除外する `.axon/.gitignore`、`* -text` の1行でGitの改行変換を止める `.axon/.gitattributes` を新規作成する。保存形式は一つで、配置や形式を選ぶoptionはない。Git内では現在のworktree rootが対象で、Git外では現在directoryが対象になる。独立試用はGit外かつ既存保存先配下ではないdirectoryで行う。

`axon init --help` と対象rootを照合し、単独commandで初期化して`axon list`で確認する。Git内で初期化した場合は、`.axon` がuntrackedであること、運用を選ぶまでは `git add -A` でcommitされること、無視する運用と追跡する運用のどちらを使うかは利用者が決め一つのrepositoryでは混ぜないことを呼出し側へ返す。`axon init`が表示する手順の案内は呼出し側が明示的に求めた場合だけ行い、ignore fileの編集、stage・commitはこのskillから実行しない。探索順、混在時の上書き、失敗境界は [保存先と復旧](../conventions/references/storage.md) を読む。既存・破損の保存先や未解決のGit indexではartifactを保全し、writerを止めて調査する。

`axon storage check [ROOT]` は探索で確定した保存先、または指定した管理rootを、条件実行や保存先の変更なしに検査し、破損・衝突・違反・gapを報告する。破損・衝突・違反があれば終了1、gapだけなら終了0。Git indexで `.axon/` の下のpathがunmergedなら、記録を検査せずに、そのpathとworktreeを示す診断だけで終了1になる。そのworktreeでのGitでの解決とstageが要ることを呼出し側へ返し、`.axon/header.json` のGitでの衝突は別々の`axon init`で作ったstoreどうしで一つの保存先にはできないことも返す。種類ごとの行がない他の `Error:` の終了1（未初期化など）は検査結果ではない。衝突と違反の解決は `axon-kit:resolve` を使う。検査結果、初期化の保存結果、残ったartifactを返す。
