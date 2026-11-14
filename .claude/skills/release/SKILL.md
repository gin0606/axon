---
name: release
description: Axonの新しいversionを公開する。前のリリースからの変更の整理、version更新のcommit、タグのpush、Release workflowの見届け、リリースノートの作成までを行う。「リリースして」「新しいversionを出して」のような依頼に使う。
---

# リリース

決めごとと失敗時の復旧は [リリース](../../../docs/development/releasing.md) に従う。

## 手順

1. mainが `origin/main` と一致し、作業ツリーに未commitの変更がないことを確かめる。
2. 前に公開したReleaseのタグからの変更を調べる。`.axon/` だけを変えるcommitは計画の記録なので除く。残りをCLI・デスクトップアプリ・plugin・配布に分け、互換性を壊す変更を挙げる。
3. 決めごとに沿ってversionを決め、変更の整理と合わせてユーザーに示し、確認を得る。
4. versionを上げるcommitを作り、PRを通さずmainへpushする。
5. そのcommitに対するmainのFull verificationの成功を待ち、`python3 scripts/release.py validate-tag vX.Y.Z` が通ることを確かめてからタグを付けてpushする。タグはrulesetで削除も付け替えもできないため、タグのcommitに原因があってReleaseが失敗すると、そのversionは使えなくなる。
6. Release workflowの全jobの成功を確かめる。失敗したら失敗時の復旧の手順に従う。タグのcommitに原因があり再実行で直らないときは、ユーザーに報告し、次のversionで出し直すかを確認する。
7. リリースノートの下書きをユーザーに示し、確認を得てから `gh release edit` で反映する。workflowが付けた自動生成のノートは置き換える。

## リリースノート

利用者が、更新するか、更新で何をすればよいかを判断できるように書く。

- 英語で書く。
- 互換性を壊す変更を `## Breaking changes` として先頭に置き、何が変わり、利用者が何をすればよいかを書く。
- 続けてCLI・デスクトップアプリ・pluginなどの節に、利用者から見える変更を書く。変更のない節は置かない。
- 末尾に `**Full Changelog**: https://github.com/gin0606/axon/compare/vPREV...vX.Y.Z` を置く。
