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
Challenge が不要なら、ブラウザを起動せずにそのまま送信します。Suno が Challenge を要求した
場合は、まずオプションの Browser Bridge 拡張機能が、普段使っている Chrome プロファイル内で
不可視の検証ウィジェットを実行します。拡張機能は待機中にはローカルリスナーだけを維持します。
検証が必要なときは、nonce に紐付いた短時間だけ存在するトップレベルの `suno.com` ポップアップを
作成する代わりに、Chrome の不可視 offscreen document 内へ nonce 付きの `suno.com` iframe を
1 つ作成します。プロバイダーの計測用に通常の viewport サイズは維持しますが、Chrome はタブ、
popup、最小化ウィンドウ、別ブラウザプロセスを作成しません。拡張機能所有の第 1 階層 iframe
だけが接続でき、予期しないナビゲーション、再読み込み、切断、ID 不一致は iframe を削除して
安全側に失敗します。トークンまたは最終エラーの後も即座に iframe を削除し、可視コンテキストへ
フォールバックしません。この動作は macOS と Windows の両方に対応します。

既定の `auto` モードが対応する Chromium 系ブラウザへフォールバックするのは、Bridge の
インストール記録がまだない場合だけです。一度 Bridge をインストールしてペアリング
すると、Bridge が利用できない場合はエラーで停止し、別のブラウザプロセスを起動しません。
別ブラウザへのフォールバックを明示的に許可する場合だけ、`challenge_browser=isolated` を
使用してください。

### macOS または Windows に Browser Bridge をインストールする

Browser Bridge は Sunox バイナリに同梱されているため、別の ZIP を用意したり、Chrome Web
Store 経由でインストールしたりする必要はありません。macOS と Windows で手順は同じです。

1. 次のコマンドで拡張機能を展開し、表示されたフォルダーを控えます。

```bash
sunox install-browser-extension
```

2. Suno で普段使っている Chrome プロファイルで `chrome://extensions` を開きます。
3. **デベロッパー モード**を有効にし、**パッケージ化されていない拡張機能を読み込む**を選んで、
   Sunox が表示したフォルダーをそのまま指定します。macOS では `~/Library` が通常は非表示のため、
   フォルダー選択画面で `Shift+Command+G` を押してパスを貼り付けます。Windows では、
   フォルダー選択画面のアドレスバーにパスを貼り付けます。
4. 拡張機能を有効にしたままにします。Suno のタブを開いておく必要はありません。

曲の作成、Challenge の実行、クレジットの消費をせずに Bridge の通信を確認できます。

```bash
sunox doctor --browser-bridge
```

拡張機能は Chrome を再起動してもそのまま利用できます。Sunox の更新に新しい Bridge が含まれて
いる場合は、次のコマンドで拡張機能のファイルを更新します。

```bash
sunox install-browser-extension --force
```

このコマンドは、生成したバンドルと展開済みのファイルを最初に比較します。再読み込みの判断には
`reload_required` だけを使用してください。これが `true` なら、ファイルが
`already_current` でも Chrome がそのランタイムをまだ確認していない可能性があるため、
Sunox Browser Bridge の拡張機能カードで **再読み込み** をクリックします。
`reload_required=false` の場合だけ、Chrome で再読み込みする必要はありません。コンピューター
または Chrome を再起動しただけで Bridge の再インストールや再読み込みが必要になることは
ありません。Suno のページを再読み込みする必要もありません。Sunox は macOS と Windows の
どちらでも、ユーザーごとのアプリケーション設定フォルダーを自動的に選びます。Chrome がこの
パッケージ化されていない拡張機能を使用している間は、そのフォルダーを移動または削除しないで
ください。

関連する上書きオプションは次のとおりです。

```text
--captcha          事前チェックで不要でもブラウザ検証を実行
--no-captcha       自動ブラウザ検証を無効化
--token <token>    外部で取得した Challenge Token を使用
```

`challenge_browser` には `auto`（既定）、`existing`（Bridge を必須とし、別ブラウザを起動しない）、
`isolated`（常に一時ブラウザを使用）を指定できます。1 回だけ上書きする場合は
`-c challenge_browser=existing` を使います。`existing` という名前は既存設定との互換性のために
残されており、現在は「既存の Chrome プロファイルにインストールされた Bridge を使う」という
意味です。Bridge は nonce に紐付いた offscreen iframe を自動的に作成して削除し、タブや
ブラウザウィンドウは開きません。設定またはペアリング済みの Bridge が見つからない、古い、または応答しない場合は、
可視コンテキストへ切り替えずエラーになります。`auto` で一時ブラウザへ
フォールバックできるのは、Bridge のインストール記録がない場合だけです。インストール済みの
Bridge が無効、古い、到達不能、またはペアリング secret が欠落している場合、`auto` もエラーで
停止します。別ブラウザを許可する
場合は、`isolated` を明示的に指定してください。

無人実行で Suno のタブを追加せず、別のブラウザプロセスも起動したくない場合は、Browser Bridge を
インストールして `--no-captcha` を外します。この状態では `auto` と
`challenge_browser=existing` のどちらも、Bridge が利用できなければエラーで停止します。
`existing` は、ペアリング情報がまだない場合でも Bridge を必須とします。Bridge が未インストール、
またはインストール済みか確認できない場合は `--no-captcha` を残してください。Challenge が必要に
なれば、送信前に停止します。Bridge が未設定の状態で、既定の `auto` から単に `--no-captcha` を
外すだけでは、一時ブラウザへのフォールバックが許可されたままです。

Browser Bridge のインストールは、Sunox が自動管理する短時間のコンテキストで Challenge を実行
することへの継続的な許可とみなされます。生成のたびに確認を取り直す必要はありません。
「Suno のタブを残さない」「新しいブラウザを起動しない」「CAPTCHA を表示しない」といった指定は、
インストール済みの Bridge の利用を許可するものであり、`--no-captcha` を意味しません。
Bridge だけに限定する明示的な設定は、引き続き `challenge_browser=existing` です。Bridge を
インストール済みでも `--no-captcha` を使うのは、Bridge を含むすべての Challenge が明示的に
禁止された場合、またはそのフラグ自体が明示された場合だけです。

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
