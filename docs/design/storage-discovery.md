# 設定ファイルを持たない保存先と初期化

2026-09-07 の合意。file-backend.md の backend 選択・配置・init・Git setup の判断を置き換える。

一つの Git repository は一つの backend を使う。worktree ごとの異種 backend 併用は
運用ミスとして非対応。全 worktree の走査や共有 selector は設けない。
SQLite は Git common directory の親の `.axon/axon.db` を共有し、Git 外では管理 root の
`.axon/axon.db` を使う。file は現在の worktree（Git 外では管理 root）の
`.axon/state.jsonl` を使う。backend 設定ファイルは廃止する。

通常操作は二つの所定の正本の存在から backend を判別する。両方あればエラー、
どちらもなければ未初期化。破損、読取不能、初期化途中では他の保存先へ fallback しない。
Git 内は現在の repository を境界にし、外側の管理 root を使わない。
Git 外は祖先を探索し最寄りの正本または初期化途中の marker で止まる。
空の `.axon` や lock だけでは管理 root としない。

`init` は新規作成専用。既存の箱があれば再実行を拒否する。既存管理 root 配下の
Git 外の入れ子 init も拒否する。backend と prefix の変更、移行、修復はしない。
SQLite を既定とし、明示的に `--backend file` を選べる。
backend 共通の初期化排他で確認と生成を保護し、既存正本を上書きしない。
新規 state は完成した temporary を正本として公開する。途中の失敗は対象 path、
失敗した処理、既に保存した artifact を報告する。自動 rollback や修復はしない。

SQLite init は ignore を変更しない。file init は Git 内外とも `.axon/.gitignore` と
root の `.gitattributes` を生成・補完する。無関係な内容を保持し、同じ設定は重複させない。
直接編集する file 内で必要な設定が競合すればエラーにする。外側の ignore によって
`.axon/` が除外されても解除せず、init 自体は失敗させない。
Git driver 登録、stage、commit は利用者が行う。`merge setup` は削除し、通常の
repository-local `git config` を導入ドキュメントに記す。追加の成功時案内は設けない。

`migrate` は元 SQLite を変更せず、新規出力先へ変換成果と backup、manifest を作る。
config は出力しない。運用先の配置・Git integration・writer の停止は手動移行で扱う。
旧 config / state.db レイアウトへの互換探索や暗黙移行は残さない。

## 保証の範囲

観測した二つの保存先での混在は拒否するが、見えない他 worktree との混在は検出を保証しない。
file が存在しない branch で init すると新しい独立 store ができる。既存 store の利用には
正本を含む branch を取り込む。Git 外の子で git init すると新しい管理境界になる。
Git と Axon の同時書込、手動切替中の利用、Git による欠落・混在の自動復旧は提供しない。
store ID は正本内で維持し、open 後の別 store への置換や merge 入力の不一致の検査に使う。

Quint の使い捨て小モデル5本で、探索・途中失敗・同時初期化・Git境界の変更を検討した。
各10,000 sampled traces、最大12 steps、seed 7。混在の不可視性、途中生成後の再init拒否、
祖先への誤fallback、同時initの競合、Git境界追加後の接続先変化を2〜4操作で再現した。
共通の排他区間、途中状態で探索停止、成功時の生成完了、Git境界優先の各性質には
反例なし。同時initの対策案のみ `--step serializedStep`。全探索やOS耐久性の証明ではない。
Rust tests で実装との適合を検査する。状態モデルの A/B/C/D の意味は変更しない。
