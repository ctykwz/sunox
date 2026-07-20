# sunox

`sunox` は、Suno をターミナルから操作するための非公式 CLI です。Rust 製の単一バイナリで、
曲の生成、ダウンロード、プレイリスト、Persona、カバー、リマスター、音声編集、アップロードを
扱えます。

[![crates.io](https://img.shields.io/crates/v/sunox)](https://crates.io/crates/sunox)
[![CI](https://github.com/ctykwz/sunox/actions/workflows/ci.yml/badge.svg)](https://github.com/ctykwz/sunox/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

[English](README.md) · [简体中文](README.zh-CN.md) · 日本語 · [Français](README.fr.md) ·
[Español](README.es.md)

> [!WARNING]
> Sunox は Suno の公式製品ではなく、Suno との提携関係もありません。Suno Web の非公開 API
> を利用しているため、予告なく動作しなくなる可能性があります。Suno の利用規約、アカウント
> の制限、生成・アップロードする素材の権利は、利用者自身で確認してください。

## 主な機能

- 短い説明、カスタム歌詞、スタイル指定、Persona、インストゥルメンタル指定から曲を生成
- 生成完了を待ち、MP3、M4A、WAV、Opus、動画をダウンロード
- 曲の一覧・検索・編集・公開・削除・復元
- カバー、延長、連結、リマスター、速度変更、反転、切り抜き、フェード、ステム生成
- プレイリストと音声 Persona の管理、ローカル音声やカバー画像のアップロード
- ターミナル向け表示と、スクリプトや Coding Agent 向け JSON 出力

Suno Studio の機能は対象外です。

## インストール

Rust 1.88 以降がある場合は Cargo からインストールできます。

```bash
cargo install sunox
```

Rust を入れたくない場合は、[GitHub Releases](https://github.com/ctykwz/sunox/releases)
から macOS、Linux、Windows 用のビルド済みバイナリを取得できます。現在の配布物には Apple
や Windows の商用署名がないため、OS の警告が表示されることがあります。各リリースには
`SHA256SUMS` が含まれ、`sunox update` は更新前にアーカイブを検証します。

## ログイン

まずブラウザで suno.com にログインし、次のコマンドを実行します。

```bash
sunox login
```

Sunox は Chrome、Edge、Brave、Arc、Chromium、Firefox の順に、再利用できるセッションを
探します。見つからない場合だけ、専用のブラウザ Profile を開いて対話的なログインを行います。

認証情報は Sunox のローカル設定ディレクトリに保存されます。Cookie や JWT をコマンドライン、
ログ、プロジェクトファイル、コミットに残さないでください。ヘッドレス環境では
`--cookie-stdin` または `--jwt-stdin` を利用します。

```bash
sunox doctor
sunox credits
```

## 曲を生成してダウンロードする

短い説明だけでも生成できます。

```bash
sunox "柔らかなシンセとゆっくりしたビートのアンビエント・エレクトロニカ"
```

歌詞や生成パラメータを指定する場合は `create` を使います。

```bash
sunox create \
  --title "Night Drive" \
  --tags "dream pop, synth, female vocal" \
  --exclude "metal, aggressive" \
  --lyrics-file lyrics.txt \
  --weirdness 35 \
  --style-influence 70
```

### インストゥルメンタルの入力モード

どちらか一方だけを選びます。`--instrumental` は `--lyrics` や `--lyrics-file` と同時に使えません。

- 歌詞なしで内部構成を細かく指定しない場合は、`--instrumental` だけを使います。
- セクション、リズム、編集点、編曲を制御する場合は `--instrumental` を付けず、先頭行が
  `[Instrumental]` の構造ファイルを `--lyrics-file` で渡します。歌唱可能な本文を残さず、
  それ以外の空でない行もすべて角括弧内に記述します。

生成後は `sunox clip timed-lyrics <clip_id> --json` を実行し、`success=true` の空でない整列語が
1 件でもあれば、その生成版を採用しません。

通常、1 回の生成で 2 つの Clip ID が返ります。完了を待ってから必要な曲をダウンロードします。

```bash
sunox clip wait <clip_id_1> <clip_id_2>
sunox download <clip_id_1> <clip_id_2> --output ./songs
```

形式を指定しない場合は既存の CDN MP3 を取得し、利用可能な通常歌詞と同期歌詞を ID3 に
書き込みます。Suno の形式変換が必要なときだけ `--format mp3|m4a|wav|opus` を、動画には
`--video` を指定してください。

## よく使うコマンド

```text
sunox <説明>                       短い説明から曲を生成
sunox create [説明]                詳細な条件を指定して生成
sunox lyrics                       歌詞だけを生成

sunox clip list                    自分の曲を一覧表示
sunox clip search <キーワード>     曲を検索
sunox clip info <id>               曲の詳細を表示
sunox clip wait <ids>              生成完了を待つ
sunox download <ids>               完成した曲をダウンロード

sunox clip cover <id>              カバーを生成
sunox clip extend <id>             曲を延長
sunox clip concat <ids>            複数の Clip を連結
sunox clip remaster <id>           リマスター
sunox clip speed <id>              再生速度を変更
sunox clip reverse <id>            音声を反転
sunox clip crop <id>               指定区間を残す、または削除
sunox clip fade <id>               フェードを追加
sunox clip stems <id>              ステムを生成

sunox playlist list                プレイリストを一覧表示
sunox playlist create              プレイリストを作成
sunox add <clip_ids> --to <id>     曲をプレイリストに追加

sunox persona list                 音声 Persona を一覧表示
sunox persona create <clip_id>     曲から Persona を作成

sunox clip upload <ファイル>       ローカル音声をアップロード
sunox models                       利用可能なモデルを表示
sunox doctor --network             DNS、TCP、HTTPS を診断
sunox update                       最新の GitHub Release に更新
```

すべてのオプションは `sunox --help` または `sunox <コマンド> --help` で確認できます。

## 生成時の Challenge

生成系のリクエストを送る前に、Sunox は Suno Web と同じ Challenge チェックを行います。
Challenge が不要ならブラウザを起動せず、そのまま送信します。Suno が要求した場合は、まず任意の
Browser Bridge 拡張機能が既存の `suno.com` タブ内で invisible challenge を実行します。ペアリング済み
タブが応答しない場合、既定の `auto` モードが対応する Chromium を使用し、終了後に一時 Profile を削除します。

Browser Bridge は Sunox バイナリに同梱され、macOS と Windows の両方に対応しています。
別の ZIP や Chrome Web Store は不要です。`sunox install-browser-extension` を実行し、表示された
パスを控えます。Suno で使う Chrome プロファイルの `chrome://extensions` を開き、デベロッパー
モードを有効にして **パッケージ化されていない拡張機能を読み込む** を選び、そのフォルダーを
指定してから、ログイン済みの `suno.com` タブを再読み込みしてください。macOS では `~/Library`
が通常は非表示のため、フォルダー選択画面で `Shift+Command+G` を押してパスを貼り付けます。
Windows では、フォルダー選択画面のアドレスバーにパスを貼り付けます。

Sunox の更新後は `sunox install-browser-extension --force` を実行し、拡張機能カードの
**再読み込み** をクリックして、Suno タブも再読み込みします。Chrome が使用している間は、
表示された拡張機能フォルダーを移動または削除しないでください。

```text
--captcha          事前チェックで不要でもブラウザ検証を実行
--no-captcha       自動ブラウザ検証を無効化
--token <token>    外部で取得した Challenge Token を使用
```

`challenge_browser` は `auto`、`existing`（新しいウィンドウを開かない）、`isolated` を選択できます。
`existing` は Bridge が応答しない場合にエラーを返し、`auto` は分離ブラウザーへフォールバックできます。

無人実行で新しいウィンドウを絶対に開かない場合は、Browser Bridge のインストール確認を基準にします。
インストール済みなら `--no-captcha` を外して `-c challenge_browser=existing` を使います。このモードが
更新済みのログイン済み Suno タブへの接続を確認し、未接続なら別ブラウザーを開かずに失敗します。
未インストールまたはインストールを確認できない場合は `--no-captcha` を残します。

## JSON と自動化

すべてのコマンドで `--json` を利用できます。stdout を Pipe した場合も自動で JSON になります。

```bash
sunox clip list --json
sunox clip list | jq '.data.clips[0].title'
sunox agent-info --json
```

複数段階の処理や一括操作が途中で失敗した場合、結果には完了済み・失敗・未実行の項目が分けて
含まれます。必要な項目だけを再試行できます。

Coding Agent 向けの利用 Skill も同梱されています。

```bash
sunox install-skill                 # Codex
sunox install-skill --target claude
sunox install-skill --target cursor
```

## 設定と安全性

```bash
sunox config show
sunox config set output_dir ./songs
sunox config set default_model auto
```

`-c key=value` は 1 回の実行だけ設定を上書きします。環境変数は `SUNOX_*` 接頭辞を使います。

同じアカウントへの書き込みは、競合を避けるため既定で直列化されます。`--parallel` はこの保護を
1 回だけ無効にするため、意図的に並列書き込みを行う場合にだけ使用してください。

一部のコマンドは Credits を消費したり、Suno 上のデータを変更したりします。新しい曲、
プレイリスト、Persona は明示的に公開しない限り非公開です。取り消せない操作には `-y` または
`--yes` が必要です。

## 開発

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

変更は `main` から機能ブランチを作り、Pull Request で提出してください。

## ライセンス

[MIT](LICENSE)
