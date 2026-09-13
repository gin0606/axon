# 候補一覧と外部条件

正本は [単一 lifecycle spec](../../spec/lifecycle_proposal.md#評価契約)。`src/lifecycle/candidates.rs` が backend に依存しない候補選択を行い、`src/condition.rs` が CLI の外部プロセスを監督する。旧 `src/derived.rs` の三軸の候補・着手判定は新 CLI から参照しない。

```sh
axon triage
axon tasks
axon list
axon when set ID --command 'test -f ready.txt'
axon tasks --condition-timeout 5s --trace-conditions
axon when clear ID
```

`triage` は自身と全祖先が浮上した未判断を表示する。`tasks` は浮上した未着手と、浮上を問わない全着手中を表示する。依存先の完了待ちや親の着手待ちの未着手も含む。一覧は ID・種別・状況・タイトルを作成日時順に表示する。`list` と `show` は条件を実行せず、保存情報を閲覧する。

条件が未設定なら成立する。候補の祖先を上から評価し、未成立の配下を省略する。kind/searchの絞り込み後に残る候補とその祖先だけを評価する。対象外の状態や終了した Entity は表示のためには評価しない。着手中の条件は子孫の候補判定で必要な場合だけ評価する。同じ Entity は一回の取得で最大一回評価し、次の取得では再評価する。一件でも必要な評価が失敗したら、着手中を含む部分一覧を stdout へ返さない。取得開始時の保存snapshot を使い、評価中は書込み transaction を保持しない。

`when set` は一つの shell 文字列を保存・置換し、`when clear` は未設定へ戻す。これらは終了済み Entity にも使え、lifecycle や履歴を変更しない。空白だけのコマンドは拒否する。設定・修復・明示した lifecycle 操作・包含・依存変更は外部条件を実行しないため、壊れた条件も修復できる。

条件は `/bin/sh -c` で実行し、標準入力を閉じ、呼び出し元の環境を継承する。対話・ログイン shell の初期化は行わない。作業場所は現在の Git worktree のルート、Git 外では管理ルート。SQLite を共有する別 worktree でも、条件を設定した場所へ戻らない。終了0は成立、1は未成立、それ以外・起動失敗・シグナル終了・timeoutは判定失敗。stdout の内容は成立判定に使わない。

`--condition-timeout` は正の整数と単位ms/s/m/hを受け付ける（例500ms・30s・2m・1h、既定30s）。各条件に同じ制限を適用し、保存値には含めない。timeout と Ctrl-C は専用 process group に TERM を送り、1秒後も残れば KILL する。同じ group の子も終了対象で、中断を成功や未成立にはしない。

正常な条件出力は通常一覧に混ぜない。失敗診断には Entity、shell 文字列、作業場所、終了理由、取得した stdout/stderr を含める。`--trace-conditions` は実際に評価した正常終了の条件の Entity、作業場所、成立可否、終了コード、stdout/stderr を評価順に stderr へ表示し、共有結果を再表示しない。空出力は `(empty)` と表示する。trace の書込み・flush が失敗しても一覧取得は失敗する。

stdout/stderr は並行して読み、各64 KiBまで保持する。超過時は先頭・末尾の各32 KiBと省略byte数を表示し、残りも読み捨てて pipe の詰まりを防ぐ。非 UTF-8 は置換表示、端末制御文字は可視 escape とする。これは秘密情報の自動除去を保証するものではない。

検証は `cargo test`。library では候補集合と評価順・共有を boolean oracle と比較し、独立 fixture の smoke では集合、修復、非実行、worktree・管理ルート、実プロセスの終了コード、30秒の既定値、timeout・Ctrl-Cと子の終了、出力上限、trace書込失敗を検査する。binary test は起動失敗とtrace flush失敗も実プロセスで検査する。モデルの意味は変更せず、spec の候補一覧モデルで検証済みの集合・省略契約へ実装を適合させる。
