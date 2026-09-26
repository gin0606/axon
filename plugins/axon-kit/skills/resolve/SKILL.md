---
name: resolve
description: Axonの保存先の衝突と構造の違反を`axon storage check`で検出し、`axon resolve`と通常操作で解消して通常操作へ戻す。Git操作の完遂、破損したfileの修復、保存先の初期化には使わない。
---

# 衝突と違反の解決

`axon-kit:conventions` を使い、指定されたbinaryと保存先を固定する。Git統合時にAxonは呼ばれず、統合の結果は次の読取で衝突・違反・記録の欠け（gap）・破損として現れる。境界は [保存先と復旧](../conventions/references/storage.md) を読む。採るheadと修復の効果は呼出し側の判断または合意済みの効果から決め、意味上の選択が未確定なら候補を返す。

1. `axon storage check` を単独で実行し、終了コードと種類ごとの行を読む。破損があれば記録からの導出は行われず全操作が止まる。報告されたpathと理由、`Hint:` の案内を返し、fileの復元（Gitの履歴からの取り出し、改行変換の解消）は呼出し側の権限で行う。gapだけなら終了0の情報で、操作は続けられる。Git indexで `.axon/` の下のpathがunmergedと報告されたら、Gitでの解決とstageが要ることを返す。`.axon/header.json` の衝突は別々の`axon init`で作ったstoreどうしの統合で、一つの保存先にはできないことも返す。
2. 衝突中のEntityがあれば、`axon resolve` と `axon note add` 以外の変更は拒否される。`axon resolve [ID]` で各headの記録ID・操作・lifecycle・種類・タイトルを読み、両側の経緯は `axon log ID` と `axon note list ID`、headの本文や関係の値は確定した保存先の記録file `.axon/records/<記録IDの先頭2文字>/<記録ID>` の `after` を読んで確かめる（fileは編集しない）。解決はheadの一つの現在値全体を採り、項目ごとの合成はできない。`parent missing; likely newer` の付いたheadはgapによる偽の衝突の新しい側である可能性が高いが、この印や日時だけで採る側を決めない。欠けた記録fileをGitの履歴から戻せる場合は、解決せずに衝突が消えることがあるので、その選択肢も返す。
3. `axon resolve ID --head RECORD_ID` を単独で実行し、完全な記録IDを渡す。呼出し側が理由を与えた場合だけ `-r` で記録する。成功出力の `+Invalid` は対象Entity自身の違反だけを示し、他のEntityに残った違反は次の手順の `axon storage check` で読む。採らなかったheadの変更は現在値に残らない。やり直すかは呼出し側の判断で、衝突がなくなってから通常操作で行う。両側で同じ衝突を別々に解決した場合は、統合後にもう一度解決する。衝突中のEntityが一つでも残る間は通常操作が拒否されるため、全Entityを解決してから次へ進む。
4. 残った違反を `axon storage check` と `axon show ID --details --skip-conditions` から読み、`axon reopen`（依存元から順に）、`axon parent unset`、`axon dep rm`、再採用などの通常操作で直す。通常操作は違反を増やせないため、拒否されたら診断に従って順序や操作を変え、Git操作や記録fileの削除で回避しない。途中で止まった `axon import apply` が残した違反は通常操作で直さず、`axon-kit:declaration` の再試行に任せる。修復が呼出し側の与えた範囲を超える違反は報告して残す。
5. `axon storage check` で衝突が残らず、残る違反が報告済みのものだけであることを確認して通常操作へ戻る。gapは欠けた記録fileをGitの履歴から戻せば埋まるが、そのままでも操作は続けられる。戻すかどうかと、そのGit操作は呼出し側の権限で行う。

Axonの状態をGitのrevertで取り消さない。記録fileのstage・commit、merge・rebaseの続行・中止は呼出し側の権限で行う。結果不明は [保存操作](../conventions/references/mutations.md) に従って照合し、`axon resolve` を推測で繰り返さない。対象、採ったhead、残った違反・gap・破損、保存結果を返す。
