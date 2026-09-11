---
name: add-note
description: 既存IssueまたはGroupへ確定した補足情報をimmutableなNoteとして追記する。
---

# 補足を追記する

`axon-kit:conventions` を使い、指定された新binaryと保存先を維持する。 本文変更・状態変更reasonと混同せず、調査結果、検証、訂正、申し送りをNoteへ残す。

最初の追記前に正確な本文bytesとdigest、`note list ID` のNote ID集合を保存する。`note add ID -m ...` または `-F snapshot` を一度だけ実行し、返されたIDを記録する。`note list ID --recorder-details` でID、本文、日時、取得できた記録者を照合する。actor/sessionは自動取得の任意情報で、追記の前提にしない。

結果不明なら元processの終了後、事前集合になかったNoteすべてを読み固定本文と比較する。actor一致だけで自分の操作と決めない。新IDと固定payload等から適用を立証できたら再追記しない。一致候補なしで未適用を確認できる場合だけ同じsnapshotで一度再試行できる。複数候補など照合不能なら不明として返し、他のNoteを保持する。
