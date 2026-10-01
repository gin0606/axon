---
name: conventions
description: Axonのlifecycle・情報モデルとCLI保存操作の共通契約。他のkitと利用側workflowの基盤に使い、対象選択、採用判断、実装、commitは規定しない。
---

# Axonの共通操作契約

## 表記の読み方

このkitと利用者側workflowでは、`axon start`のような`axon`付きのcode表記はAxonのコマンド、`Start`・`Release`のようなCamelCaseのcode表記は遷移概念、`InProgress`・`Completed`は状態を指す。遷移は動詞形（`Complete`）、状態は過去分詞形（`Completed`）で区別する。コマンドの一部である段階名（`prepare`・`check`・`apply`）、フラグ（`--details`）、引数名（`parent`・`needs`）を単独で指す場合もcode表記にする。

code表記でない英単語はAxonの操作を指さない。field名、記録の名詞、他ツールの識別子などの場合がある。一般的な作業の中断・完了・統合・リリースは日本語で書く。この表記規約はリポジトリが書く日本語の文書・skillを対象とし、英語のCLI出力・help・内蔵文書には及ばない。

## 対象環境を固定する

呼び出し側が指定したrepositoryまたは管理rootで操作する。binaryが指定されていればそれを使い、指定がなければその環境の `axon` を発見し、`axon --version` と `axon --help` を確認する。`axon tasks|accept|cancel|reopen|resolve|convert|label`・`axon storage check` の有無と必要なleaf helpの構文をこのpluginの契約と照合して、実行ファイルとworking directoryを以後の操作で固定する。別repositoryのソースcheckoutは不要。このplugin内のreferenceと選択したCLIの `axon docs`・helpで手順を完結させる。

コマンドやschemaが非対応なら観測したversion・構文・対象を返す。別のbinaryへの切替や、記録fileやheaderの直接編集で操作を代替しない。実保存先の境界は [保存操作](references/mutations.md) に従う。

## 必要な契約を読む

- Entity・候補・情報を解釈する前に [モデルと参照](references/model.md)。
- mutation前に [保存操作と再試行](references/mutations.md)。
- 新規登録前に [作成と照合](references/creation.md)。
- 初期化、統合後の検査、file障害では [保存先と復旧](references/storage.md)。

対象、採用判断、作業選択、実装・review・commitの権限は呼び出し側workflowが与える。CLIが操作を受け付けることも権限の根拠ではない。与えられた効果は重ねて確認せず実行できるが、意味を変える未確定な判断は呼び出し側へ返す。

構文の不確実さは保存先不要のhelpで解決する。保存情報は `axon list`・`axon show ID --details --skip-conditions`・`axon log`・`axon note` で調べ、外部条件の評価が必要なときだけ `axon proposals|tasks` と既定の `axon show` を使う。`axon show` は `--skip-conditions` を付けない限り対象の状況に必要な条件を実行するため、操作前の現在値確認や本文の取得には `--skip-conditions` を付け、状況の診断（浮上していない候補や祖先を含む詰まっている理由）には付けない。任意recorderは認証・lock・生存確認ではない。

要求した作用、完全ID、保存結果（適用済み・未適用・部分適用・不明）、最終状態と関係への影響を返す。保存結果は読み取り操作と保全したartifactの照合で確認できるため、確認のためだけの追加mutationはしない。
