# Axonを組み込むワークフローのサンプル

`axon-workflow`は、計画を作り、IssueやGroupの作業を完遂するpluginです。そのまま試したり、コピーして自分の計画・検証・commitの手順に合わせて変更したりできます。Claude CodeとCodexで共通のskillを使います。

| Skill | 動作 |
| --- | --- |
| `axon-workflow:plan` | 簡単な計画を作り、確認後に登録・更新。新規Entityは未判断で登録 |
| `axon-workflow:start` | 指定Issueを実装・検証し、完了記録とcommit。Groupの子Issueも指定可能 |
| `axon-workflow:start-group` | 配下のIssueを依存順に一件ずつ進め、子Groupと全体の完了条件を確認して完了 |

実行系は既定でcommitまで進めます。ユーザーの指示と利用先の規約で変更できます。独立レビューや別エージェントによる終了監査は必須にせず、必要なテストと完了条件の確認を行います。利用先が要求するレビューは引き続き適用されます。

## 導入

対応するAxon CLIと、同じmarketplaceの`axon-kit`・`axon` pluginが必要です。その他のworkflow pluginには依存しません。実装・commitを試す対象はGit repositoryを想定しています。CLIと保存先の準備は[導入ガイド](../../docs/guide/getting-started.md)を参照してください。

このcheckoutのルートで、利用するホストに応じて実行します。既にmarketplaceや依存pluginを導入している場合は、必要な追加・更新だけを行ってください。

Claude Code:

```sh
claude plugin marketplace add .
claude plugin install axon-kit@axon --scope user
claude plugin install axon@axon --scope user
claude plugin install axon-workflow@axon --scope user
```

Codex:

```sh
codex plugin marketplace add .
codex plugin add axon-kit@axon
codex plugin add axon@axon
codex plugin add axon-workflow@axon
```

導入後、新しいセッションを対象repositoryで開始します。サンプルを変更した場合は、利用するホストのplugin更新手順で読み込み直してください。

## 一巡させる

最初は試用用repositoryと独立したAxon保存先で、小さな変更から試せます。以下はCodexの呼び出し例です。Claude Codeでは先頭の`$`を`/`に置き換えます。

```text
$axon-workflow:plan READMEに開発用コマンドの説明を追加する計画を作って
```

提示された内容を確認すると、未判断の計画が登録されます。実装する内容が決まったら、対象を明示して採用を依頼します。Groupでは、実行する子Issue・子Groupも採用が必要です。採用には既存の`axon:triage`を利用できます。計画の登録だけでは実装は始まりません。

```text
$axon-workflow:start <採用済みIssueのID>
$axon-workflow:start-group <採用済みGroupのID>
```

`start`は指定Issueだけ、`start-group`は指定Group全体を実行します。どちらも既定でcommitするため、完了時に変更履歴を確認できます。未commitで確認したい場合は、呼び出しに「commitせず差分を残して」と添えてください。

## 自分の運用に合わせる

- [plan](skills/plan/SKILL.md): 計画の作り方や本文の書式を変更する。
- [start](skills/start/SKILL.md): 実装・検証の手順や、必要ならレビューを追加する。
- [start-group](skills/start-group/SKILL.md): Issueの実行順とGroup全体の確認方法を変更する。
- [実行の共通手順](references/execution.md): commit・継続・中断の方針を変更する。

コピーして独自pluginとして配布する場合は、両manifestの名前とskill内の`axon-workflow:`参照を変更します。`axon`を呼ぶ箇所ではその協業方針に従います。協業方針自体を変えたい箇所は、`axon-kit`の操作契約に従う手順へ置き換えてください。

## 再浮上条件とcacheexec

外部APIへの問い合わせなど、時間のかかる再浮上条件は、`axon tasks`などの表示を遅くするため、結果をキャッシュしてください。利用できるキャッシュ手段がなければ、Axonと同じ作者が提供する[cacheexec](https://github.com/gin0606/cacheexec)をおすすめします。

```sh
brew install gin0606/tap/cacheexec
```

条件コマンドの例:

```sh
cacheexec --ttl 5m --include-codes 0,1 -- ./scripts/check-ready.sh
```

`check-ready.sh`は利用先が用意する条件スクリプトです。成立を`0`、未成立を`1`、通信失敗などの判定エラーをそれ以外の終了コードで返します。この例では成立・未成立だけを5分間再利用し、判定エラーはキャッシュしません。検知までに許容できる遅延に合わせてTTLを調整します。条件の設定は`axon:triage`で扱います。
