# 状態と用語

Issueは仕事や懸念、Groupは子を持つ計画です。各Entityは一つのlifecycleを持ちます。

| lifecycle | 意味 |
| --- | --- |
| `Undecided` | 未判断 |
| `NotStarted` | 採用済み・未着手 |
| `InProgress` | 着手中 |
| `Completed` | 完了 |
| `Cancelled` | 取りやめ |

`Completed`と`Cancelled`は終了です。`Cancelled`への依存は満たされず、`Completed`への依存だけが満たされます。子の`Start`には親の`InProgress`が必要です。着手中でも依存が未完了になれば、`axon tasks`で依存待ちが分かります。

再浮上条件は未設定または外部コマンドです。候補一覧への浮上と明示操作の可否は別で、条件だけで`Start`を拒否しません。本文は現在の計画、Noteは追記する補足、logは状態変更・統合の履歴です。記録者は任意の付随情報で、権限や排他には使いません。

詳しい契約は [正本spec](../../spec/lifecycle_proposal.md)、操作例は [日常の操作](usage.md) にあります。
