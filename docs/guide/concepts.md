# 状態と用語

Issueは仕事や懸念、Groupは子を持つ計画です。各Entityは一つのlifecycleを持ちます。種類は現在値で、未判断・未着手のIssueとGroupは `axon convert` で相互に変換できます（Groupは子がない場合）。

| lifecycle | 意味 |
| --- | --- |
| `Undecided` | 未判断 |
| `NotStarted` | 採用済み・未着手 |
| `InProgress` | 着手中 |
| `Completed` | 完了 |
| `Cancelled` | 取りやめ |

`Completed`と`Cancelled`は終了です。`Cancelled`は`Reconsider`で`Undecided`へ、`Completed`は`Reopen`で`NotStarted`へ戻せます。`Cancelled`への依存は満たされず、`Completed`への依存だけが満たされます。

着手（`Start`）と解放（`Release`）はIssueだけの操作です。Issueの`Start`には、全祖先が採用済み（`NotStarted`）で、自身と全祖先の依存先が`Completed`であることが必要です。Groupは着手を記録せず、配下のIssueが着手・完了すると実効lifecycleが`InProgress`になります。Groupは最終確認として`axon complete`で完了させます。着手中でも依存が未完了になれば、`axon tasks`で依存待ちが分かります。

再浮上条件は未設定または外部コマンドです。候補一覧への浮上と明示操作の可否は別で、条件だけで`Start`を拒否しません。本文は現在の計画、Noteは追記する補足、logは状態変更・統合の履歴です。記録者は任意の付随情報で、権限や排他には使いません。

詳しい契約は [lifecycleと構造の契約](../reference/lifecycle.md)、計画の一括登録・編集は [Declaration](../reference/declaration.md)、操作例は [日常の操作](usage.md) にあります。
