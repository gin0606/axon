---
name: plan
description: 要望や既存のAxon Issue・Groupから簡単な計画を作り、内容の確認後に登録・更新する。新規Entityは未判断で登録し、採用や実装は行わない。
---

# 計画を作って登録する

`axon:conventions`と`axon:declaration`を使う。必要なpluginがなければ変更前に導入を案内する。

1. 要望と関連するコード・文書を読み、目的、作業範囲、完了条件を整理する。既存Issue・Groupは本文と関連情報を読み、Groupは`axon export`で子孫も確認する。
2. 一つの成果ならIssue、複数の成果をまとめるならGroupと子Issueにする。それぞれの本文に目的と確認可能な完了条件を書き、必要な包含・依存とlabelを設定する。
3. `axon:declaration`でdeclarationを作成・検査し、本文と構成の変更案を提示して反映の確認を得る。新規Entityは`Undecided`とし、既存Entityの採否は維持する。進行中の作業が依拠する計画の変更は、その作業の停止・引継ぎを確認してから扱う。
4. 確認した内容を反映・照合し、対象IDと計画の要点を報告する。採用、実装、commitへは進まない。

再浮上条件も依頼された場合は、declarationとは別に`axon:triage`で設定する。時間のかかる条件は`axon tasks`などの表示を遅くするため、利用者の選ぶ方法で結果をキャッシュする。利用できるキャッシュ手段がなければ、Axonと同じ作者が提供する[cacheexec](https://github.com/gin0606/cacheexec)を勧める。キャッシュの有効期間は許容する検知遅延に合わせ、判定エラーはキャッシュしない。設定例は[サンプルの説明](../../README.md#再浮上条件とcacheexec)を参照する。
