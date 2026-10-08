# 記録者の取得と参照

仕様の正本は [記録者情報](../reference/lifecycle.md#記録者情報とエージェント連携)。`crates/axon-recorder` は Axon core に依存しない Rust crate で、継承された環境だけから任意の actor と文字列 metadata を返す。CLI がこれを core の任意の `{actor, data}` に変換し、登録・状態変更・Note の Context に添える。core と保存 adapter は agent schema、環境検出、権限判断を持たない。

## 取得の優先順位

空白のみ・空・非UTF-8の値は取得不能として無視する。値は利用可能ならそのまま保持する。

| 順位 | 環境 | actor | data |
| --- | --- | --- | --- |
| 1 | `AXON_ACTOR` | 指定値 | `AXON_SESSION_ID` があれば `session_id` |
| 2 | `CODEX_THREAD_ID` | `codex` | `session_id` に指定値 |
| 3 | `CODEX_SANDBOX` | `codex` | 空 object |
| 4 | `CLAUDECODE` または `CLAUDE_CODE` | `claude-code` | `CLAUDE_CODE_SESSION_ID` があれば `session_id` |
| 5 | `AI_AGENT` | 指定値 | 空 object |
| 6 | `USER` | 指定値 | 空 object |
| なし | 上記の情報なし | 記録者を省略 | — |

これらはこの連携の入力契約で、すべての agent 環境で設定される保証ではない。明示 actor を優先した場合は別 agent の session を混ぜない。通常の Codex 呼出しは継承した thread ID を自動取得し、操作のたびの flag 指定は不要。`CLAUDE_CODE_SESSION_ID` は Claude Code の公開する環境変数一覧にない値で、取得できなければ actor のみを残し、この値だけでは Claude Code と判定しない。いずれの値も継承された環境をそのまま読むため、agent が起動して残った shell や tmux からの後日の操作にも同じ actor と session が付く。取得不能は操作の失敗理由にしない。環境全体、資格情報、ログファイル、プロセス一覧、ネットワークを探索しない。

## 詳細を読む

`axon log ID --recorder-details` と `axon note list ID --recorder-details` は保存時点の actor に加え `data: {...}` をJSON表記で併記する。通常表示は actor のみ。取得し直すのは保存された付随情報であり、現在の session を再探索して書き換える操作ではない。未知 actor・任意の object も core が保持した内容を表示する。記録者なしは `—`、actor のみ取得できた記録の詳細は `data: {}`。

日時が一致しても記録は別IDで保存する。記録者は状態遷移の許可、排他、生存確認、再開保証には使わない。検証はcrate単体と独立fixtureで行い、実管理データを使わない。lifecycleや関係の意味を変えないためQuintの状態追加は行わない。
