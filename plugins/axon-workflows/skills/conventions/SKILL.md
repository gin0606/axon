---
name: conventions
description: Axon Entityを起点に実装、検証、review、commitなどの外部作業を完遂するworkflowの共通規約。明示的なworkflowが与える権限、自律実行、重要な意思決定、中断時の扱いを定める。
---

# Axon完遂ワークフローの共通規約

このpluginのskillは、Axonの状態操作に閉じず、リポジトリや外部成果物への作業を終端まで進めるオーケストレーターである。Axonの意味論は`axon-kit:conventions`、個人用の判断と操作方針は`axon:conventions`を正とし、それぞれの操作に対応するskillを使う。

## 明示されたworkflowの権限を使う

workflow名を指定した呼び出し、または対象と終端を含む完全workflowの明示的な依頼だけを実行権限とする。単なる分析、着手、実装の依頼を、commitやAxon上の完了まで広げない。

workflowの呼び出しは、そのskillが列挙する対象選択、外部変更、検証、review、commit、Axon操作を、宣言された終端まで進める権限を与える。リポジトリの指示、ホストの権限制約、明示されていない外部作用は引き続き適用する。

## 自律実行と重要な意思決定を分ける

目的、scope、完了条件が確定していれば、その達成に必要な調査、実装設計、変更、検証、review findingの解決、安全な再試行を自律して行う。上位workflowが選択範囲を明示していれば、その範囲内のEntityを選べる。

目的、scope、完了条件、公開仕様、採用方針、後戻りしにくい選択を変える場合、または複数の妥当な選択肢からプロダクト上の価値判断が必要な場合はユーザーへ返す。通常の実装詳細や操作の技術的な大きさだけを停止理由にしない。

## 外部作業とAxon状態を同期する

外部成果物を変更する前に対象のclaimを取得し、同じactorとworktreeの既存claimなら再開する。別のactorまたはworktreeのclaimを奪わない。AxonのProgress、Disposition、Resurface condition、declaration、Noteは対応する`axon`または`axon-kit` skillを通して扱う。

固定したscopeに属する変更だけを成果物とcommitへ含め、無関係なworking treeの変更を保持する。分離できない既存変更がある場合は、勝手に含めたり破棄したりせず判断を返す。

完了条件とworkflow固有の検証が客観的に満たされた場合は、結果を必要なNoteへ残し、Axonのdoneと波及確認まで進める。主観的または未定義の完了条件はユーザーへ返す。

## 中断と失敗を扱う

安全でscope内にある途中成果は保持し、自動的に破棄しない。同じ会話でユーザー判断を待つ間は、workflowが別に定めない限りclaimを維持する。明示的な保留、引き継ぎ、または長期の外部待ちでは、必要な申し送りを一度だけ記録してreleaseする。

未検証またはreview未完了の成果をcommitせず、完了条件を満たさないEntityをdoneにしない。結果不明、競合、rollback、補償操作、別の最終状態が必要な場合は、観測済みの状態と必要な判断を示して停止する。

push、Pull Request、release、workflowの範囲外にある別Entityへの着手は、個別skillが明示的に含めない限り実行しない。
