---
name: work-state
description: start、release、done によって Entity の Progress と claim を同期する。明示的な lifecycle 操作、または外部作業を所有する workflow から使い、実装、作業選択、Disposition 判断、commit は行わない。
---

# Axon の作業状態を同期する

`axon-kit:conventions` を使う。lifecycle mutation の前に、そのモデルと mutation contract を読む。Note は呼び出し側の依頼または workflow が所有する別の情報操作である。

この capability は `start`、`release`、`done` による Progress と claim だけを変更する。Axon 外の実装、調査、計画、review、commit workflow は呼び出し側が所有する。

## Start

Entity context を読み、対象自身が `ready` であることを検証する。`axon start` は 1 transaction で readiness を検査し claim を取得する。

`axon start <id>` を単独の mutation として実行する。`Progress=InProgress`、観測した claim owner と worktree、start によって開いた Group frontier を検証する。

Entity がすでに `InProgress` の場合、その claim を奪ったり release したりしない。呼び出し側 workflow が互換性のある作業を再開するか判断できるよう、現在の owner を返す。対象が Undecided、Rejected、blocked、orphaned、active scope 外、ended、または他で claim されている場合、別の軸を変更したり別 Entity を選んだりせず、正確な状態を返す。

Group の start は自身の activation gate だけを開き、descendant を開始しない。child を選ばず、新しく現れた `ready` と `triage` frontier を返す。

## Release

Release は `InProgress` の Entity を `NotStarted` に戻し、claim を削除する。呼び出し側が release の依頼と任意の reason を与える。release reason は progress history に属し、補足の handoff は別の Note とする。

Group では `InProgress` の descendant が 0 件であることを検証する。副作用として descendant を release しない。

reason が与えられた場合は `-r <reason>` を付け、`axon release <id>` を単独の mutation として実行する。`Progress=NotStarted`、claim の削除、progress history、関係する frontier の変化を検証する。

## Done

Done は、この Entity に対してこれ以上作業しないことを意味する。呼び出し側がその lifecycle 判断を与え、この capability は外部 workflow の完了基準を定めない。

Group では、すべての descendant が terminal であることを要求する。descendant を変更せず、最後の child が terminal になったとき Group を自動的に完了しない。

`done` の前に、dependency、Group ancestry、呼び出し側が必要とする waiter への影響を調査する。逆方向の `AfterEntity` の完全な影響が必要な場合だけ `axon list --skip-command-evaluation` を使い、その後、関係する frontier を別途評価する。

`axon done <id>` を単独の mutation として実行する。対象の ended 状態、claim の削除、progress history、direct dependent、関係する waiter、Group ancestry、frontier への影響を検証する。ancestor Group を自動的に完了せず、完了可能になった ancestor を返す。

## 結果を返す

要求された lifecycle の作用、最終的な Progress と claim、関係する frontier と関係への影響、該当する場合は変更された storage artifact、storage 結果の分類を返す。独自に外部作業や後続の lifecycle 作用へ進まない。
