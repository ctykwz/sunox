<div align="center">

# sunox

**ターミナルから AI 音楽を生成する、Suno Web ワークフロー対応 CLI**

<br />

[![GitHub](https://img.shields.io/badge/GitHub-ctykwz%2Fsunox-181717?style=for-the-badge&logo=github)](https://github.com/ctykwz/sunox)

<br />

[![License: MIT](https://img.shields.io/badge/License-MIT-blue?style=for-the-badge)](LICENSE)
&nbsp;
[![Rust](https://img.shields.io/badge/Rust-2024-orange?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
&nbsp;
[![crates.io](https://img.shields.io/crates/v/sunox?style=for-the-badge)](https://crates.io/crates/sunox)
&nbsp;
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen?style=for-the-badge)](https://github.com/ctykwz/sunox/pulls)

---

`sunox` は Suno の Web エンドポイントを直接扱う単一バイナリの Rust CLI です。カスタム歌詞、スタイルタグ、自分の voice persona、ボーカル制御、weirdness/style スライダー、cover、remaster、speed edit、stems 抽出に対応し、ダウンロード時には MP3 に歌詞タグを自動で埋め込みます。

**Languages:** [English](README.md) | [简体中文](README.zh-CN.md) | 日本語 | [Français](README.fr.md) | [Español](README.es.md)

[インストール](#インストール) | [クイックスタート](#クイックスタート) | [人向けコマンド](#人向けコマンド) | [Agent と高度なコマンド](#agent-と高度なコマンド) | [機能](#機能) | [コントリビューション](#コントリビューション)

</div>

## なぜ作ったか

Suno の Web UI は手動操作には便利ですが、スクリプト化、ファイルからの歌詞入力、バッチ生成、ターミナル中心の音楽ワークフローには向いていません。

`sunox` はブラウザからの自動認証、主要生成パラメータの CLI flag 化、人間向け出力と JSON 出力の両対応、MP3 ダウンロード時の同期歌詞埋め込みを提供します。

Sunox は非公式プロジェクトであり、Suno との提携や承認関係はありません。非公開の Web API は予告なく変更される可能性があります。Suno の利用条件、アカウント制限、生成またはアップロードする素材の権利を守る責任は利用者にあります。

## インストール

### Cargo

```bash
cargo install sunox
```

Rust 1.88 以降が必要です。

### ビルド済みバイナリ

[GitHub Releases](https://github.com/ctykwz/sunox/releases) から macOS、Linux、Windows 向けバイナリをダウンロードできます。
各 release には `SHA256SUMS` が含まれ、`sunox update` はインストール前に対象 archive を検証します。

### セルフアップデート

```bash
sunox update --check    # 利用可能な更新を確認
sunox update            # 最新 release をインストール
```

Suno が Web schema を変更した場合は、まず `sunox update` を実行してください。パッケージマネージャの更新を待つより早いことが多いです。

## クイックスタート

```bash
# 1. ログイン。Chrome / Arc / Brave / Firefox / Edge から認証情報を自動抽出
sunox login

# 2. 自然文プロンプトで生成
sunox "rainy morning の chill lo-fi track"

# 3. 詳細パラメータを指定して生成
sunox create \
  --title "Weekend Code" \
  --tags "indie rock, guitar, upbeat" \
  --exclude "metal, heavy" \
  --lyrics-file lyrics.txt \
  --vocal male \
  --weirdness 40 \
  --style-influence 65

# 4. 返された clip ID の完了を待ってからダウンロード
sunox clip wait <clip_id_1> <clip_id_2>
sunox download <clip_id_1> <clip_id_2> --output ./songs/

# 5. プレイリストに追加
sunox add <clip_id> --to <playlist_id>
```

Agent やスクリプトは、まず `sunox agent-info --json` で機械可読な能力情報を読み、その後 `--json` 付きでリソースコマンドを呼び出してください。

## グローバルオプション

| オプション | 説明 |
|---|---|
| `--json` | 構造化 JSON を強制出力。stdout が pipe されると自動で JSON になります |
| `--quiet` | 不要な進捗出力を抑制 |
| `--parallel` | 同じアカウントで Suno 書き込みを並行実行することを許可。デフォルトはアカウント単位の直列実行 |
| `-c key=value` / `--config key=value` | その実行だけ設定を上書き。例: `-c default_model=v5.5 -c output_dir=./songs` |
| `-V` / `--version` | バージョンを表示 |
| `-h` / `--help` | コマンドまたはサブコマンドのヘルプ |

Suno の書き込み操作はデフォルトでアカウント単位に直列実行されます。
`sunox config set serial_mutations false` で永続的に無効化でき、
`-c serial_mutations=false` または `--parallel` でその実行だけ無効化できます。
環境変数による上書きは `SUNOX_*` prefix を使います。例: `SUNOX_DEFAULT_MODEL`、`SUNOX_OUTPUT_DIR`、`SUNOX_BROWSER_PATH`。

## 人向けコマンド

日常利用では主に次の入口だけを使います。

```text
sunox <prompt>                  説明文から直接生成
sunox create [prompt]           タイトル、タグ、歌詞、モデル、persona 付きで生成
sunox download <clip_ids>       完成した曲をダウンロード
sunox add <clip_ids> --to <id>  曲をプレイリストに追加
sunox login                     ブラウザから認証を設定
sunox logout                    ローカル認証情報と対話型 login profile を削除
sunox doctor                    設定と認証を診断
```

## Agent と高度なコマンド

`sunox` は Codex 風の agent、自動化、デバッグ向けに Suno リソース操作を低レベルで公開しています。Agent は `--json` を優先し、`sunox agent-info --json` で現在の契約を確認してください。

### 作成と変換

```text
sunox create              説明モードまたはカスタム歌詞モード
sunox lyrics              歌詞のみ生成。credits は消費しません
sunox clip extend         指定時刻から続きを生成
sunox clip concat         複数 clip を 1 曲に結合
sunox clip cover          別スタイルまたは別モデルで cover を作成
sunox clip inspire        既存の 1 clip を緩やかな inspiration として新曲を生成
sunox clip remaster       別モデルで remaster
sunox clip speed          再生速度を調整
sunox clip reverse        音声を反転
sunox clip crop           一部を切り出す、または中間部分を削除
sunox clip fade           フェードイン/アウトを追加
sunox clip stems          既存 clip から stems を生成
```

### 参照

```text
sunox clip list
sunox clip list --trashed
sunox clip list --liked --public --sort popular
sunox clip search <query>
sunox clip info <id>
sunox clip status <ids>
sunox clip wait <ids>
sunox persona list
sunox persona info <id>
sunox persona clips <id>
sunox playlist list
sunox playlist info <id>
sunox credits
sunox models
```

### 管理

```text
sunox download <ids>       デフォルトは CDN MP3。明示的に --format mp3|m4a|wav|opus を指定可能
sunox clip download <ids>  download と同等の agent/高度なコマンド
sunox clip upload <file>
sunox clip upload-status <upload_id>
sunox clip delete <ids> -y
sunox clip restore <ids>
sunox clip purge <ids> -y       # ゴミ箱内の曲を完全削除（元に戻せません）
sunox clip empty-trash -y       # ゴミ箱を空にする（元に戻せません）
sunox clip like <ids>
sunox clip dislike <ids>
sunox clip set <id>
sunox clip set <id> --image-file ./cover.png
sunox clip set <id> --image-url <cover_url>
sunox clip set <id> --remove-video-cover
sunox clip publish <ids>
sunox add <clip_ids> --to <playlist_id>
sunox playlist add <playlist_id> <clip_ids>
sunox playlist remove <playlist_id> <clip_ids>
sunox playlist publish <playlist_id>
sunox playlist reorder <playlist_id> --clip-id <clip_id> --index 0
sunox playlist save <playlist_id>
sunox playlist unsave <playlist_id>
sunox playlist delete <playlist_id> -y
```

### 設定と認証

```text
sunox login
sunox logout
sunox auth
sunox config
sunox doctor
sunox agent-info
sunox install-skill
sunox update
```

## 機能

### 手間の少ない認証

```bash
sunox login
```

`sunox login` はまず Chrome、Arc、Brave、Firefox、Edge から Clerk cookie を読み取ります。成功した場合はブラウザ種別と、取得できる公開 profile 設定（受け入れ言語など）を保存しますが、ブラウザ名だけから user-agent を作ることはありません。失敗した場合は Sunox 専用の Chrome/Edge 互換ブラウザ profile を開き、そこで Suno にログインすると Clerk session を取得します。その session を JWT に交換し、更新可能なローカル session として保存します。対話型 login では user-agent と受け入れ言語も取得し、API request では選択された user-agent から Chromium client hints を派生し、browser fetch metadata header を送り、実値がない項目だけ fallback します。

認証情報は OS keychain ではなくローカル JSON に保存されます。Unix では認証ファイルを `0600` で作成し、Windows では設定ディレクトリのユーザー ACL に依存します。`--cookie` と `--jwt` の値は shell history やプロセス一覧に表示される可能性があるため、対話環境では `sunox login` を優先し、認証情報をログ、prompt、プロジェクトファイル、commit に含めないでください。

認証方式：

1. `sunox login`：ブラウザから自動抽出。失敗時は対話型 Chrome/Edge login に fallback。推奨。
2. `sunox auth --cookie <cookie>`：ヘッドレス環境向けに cookie を手動指定。
3. `sunox auth --jwt <token>`：JWT を直接指定。通常は約 1 時間有効。
4. `sunox auth --refresh`：保存済み Clerk session から JWT を強制更新。

`sunox logout` はローカル認証情報と対話型 login 用の専用ブラウザ profile を削除します。

### 生成パラメータ

| Flag | 説明 | 値 |
|---|---|---|
| `--title` | 曲名 | 最大 100 文字 |
| `--tags` | スタイル指定 | モデルとアカウントの上限。`sunox models --json` で確認 |
| `--enhance-tags` | 送信前に Suno Web の tag upsample でスタイルタグを強化 | 明示的に指定 |
| `--exclude` | 避けたいスタイル | モデルとアカウントの上限。`sunox models --json` で確認 |
| `--lyrics` / `--lyrics-file` | カスタム歌詞 | `max_lengths.gpt_description_prompt` |
| `--prompt` | description mode の prompt | `max_lengths.prompt` |
| `--model` | モデルバージョン | v5.5, v5, v4.5+, v4.5-all, v4.5, v4, v3.5, v3, v2 |
| `--vocal` | ボーカル性別 | male, female |
| `--persona` | Voice persona ID | Suno の voice UUID |
| `--weirdness` | 実験度 | 0-100 |
| `--style-influence` | スタイル追従度 | 0-100 |
| `--instrumental` | インストゥルメンタル | flag |

### Voice Personas

```bash
sunox persona list
sunox persona info <persona_id>
sunox persona create <clip_id> --name "My Voice" --description "Warm lead vocal"
sunox create --persona <persona_id> --title "My Song" --tags "pop" --lyrics "[Verse]\nHello world"
```

公開、非公開、favorite、削除、復元、完全削除もできます。

```bash
sunox persona publish <persona_id>        # 明示的に公開したい場合のみ
sunox persona unpublish <persona_id>
sunox persona love <persona_id>
sunox persona unlove <persona_id>
sunox persona delete <persona_id> -y
sunox persona restore <persona_id>
sunox persona purge <persona_id> -y       # 完全削除
```

### プレイリスト

```bash
sunox playlist list
sunox playlist create --name "Release candidates" --description "Tracks to review"
sunox add <clip_id_1> <clip_id_2> --to <playlist_id>
sunox playlist remove <playlist_id> <clip_id_1>
sunox playlist publish <playlist_id> --private
sunox playlist reorder <playlist_id> --clip-id <clip_id> --index 0
```

### Clip 変換

```bash
# 以下のコマンドは submitted/processing の clip を返す場合があります。後続処理の前に wait します
sunox clip cover <clip_id> --tags "jazz, smooth piano" --model v5.5
sunox clip inspire <clip_id> --title "New Song" --tags "garage pop" --lyrics-file lyrics.txt
sunox clip remaster <clip_id> --model v5.5
sunox clip speed <clip_id> --multiplier 0.94
sunox clip reverse <clip_id>
sunox clip wait <new_clip_id>
sunox download <new_clip_id> --output ./remastered/

# crop/fade は結果 clip の complete まで内部で待つため、再度 wait する必要はありません
sunox clip crop <clip_id> --start 12.5 --end 74.0
sunox clip crop <clip_id> --start 30.0 --end 45.0 --remove-section
sunox clip fade <clip_id> --in 2.0 --out 78.5
```

### 歌詞付きダウンロード

MP3 ダウンロード時に次の ID3 タグを自動で埋め込みます。

- **USLT**：通常歌詞。
- **SYLT**：単語単位の同期歌詞。

```bash
sunox download <id1> <id2> --output ./songs/

# 同名の既存ファイルを上書きする場合だけ --force を明示する
sunox download <id1> --output ./songs/ --force
sunox download <id1> --format wav --output ./songs/
sunox download <id1> --video --output ./videos/
```

ファイル名は `title-slug-clipid8.<ext>` 形式です。出力ディレクトリは自動作成され、既存ファイルはデフォルトで保持されます。上書きするのは `--force` を明示した場合だけです。

### 音声アップロード

```bash
sunox clip upload ./demo.mp3 --title "Demo Upload"
sunox clip upload ./demo.wav --lyrics-file lyrics.txt --timeout 900
sunox clip upload ./vocal-stem.wav --stem-mix --title "Vocal stem"
sunox clip upload-status <upload_id> --json  # 読み取り専用。upload mutation は再実行しません
```

## モデル

| Version | Codename | Notes |
|---|---|---|
| auto | account response | CLI デフォルト。現在のアカウントで利用可能なデフォルトを選択 |
| v5.5 | chirp-fenix | 最新世代。billing を取得できない場合のみ fallback |
| v5 | chirp-crow | 前世代 |
| v4.5+ | chirp-bluejay | 拡張機能 |
| v4.5-all | chirp-auk-turbo | アカウントで提供される場合の無料枠モデル |
| v4.5 | chirp-auk | 安定版 |
| v4 | chirp-v4 | 旧版 |
| v3.5 | chirp-v3-5 | 旧版 |
| v3 | chirp-v3-0 | 旧版 |
| v2 | chirp-v2-xxl-alpha | 旧版 |

Remaster models: v5.5 = chirp-flounder, v5 = chirp-carp, v4.5+ = chirp-bass。

モデルの利用可否、アカウントのデフォルト、文字数上限はアカウントごとに異なります。`default_model=auto` は `/api/billing/info/` から利用可能なデフォルトを直接選び、`sunox models --json` は同じアカウント情報を確認するために使います。明示したモデルは billing 情報がある場合に `can_use` と `max_lengths` を検証し、取得できない場合のみ v5.5 に fallback します。

## Agent 向け出力

- すべてのコマンドが `--json` をサポートします。
- stdout が pipe されると自動で JSON になります。
- 進捗とエラーは stderr に出るため JSON を汚しません。
- Suno の書き込み操作はデフォルトでアカウント単位に直列実行されます。ユーザーが同一アカウントの並行書き込みを明示的に許可していない限り、`sunox config set serial_mutations false`、`-c serial_mutations=false`、または `--parallel` は使わないでください。
- 通常の音声確認では既存の clip メディアを使います。`sunox clip info <id> --json` は `audio_url` に加えて `attribution`、`comments`、`direct_children_count`、`similar_clips` を返します。認証やレート制限以外の補足読み取りが失敗しても base clip は返り、JSON には `supplemental_errors` が入ります。認証とレート制限のエラーは通常どおり中断します。`sunox clip download` はデフォルトでその `audio_url` の CDN MP3 をダウンロードして歌詞を埋め込みます。明示的な `--format mp3|m4a|wav|opus` は Suno の公式形式を要求し、`--video` は `clip.video_url` がある場合のみ使います。`sunox clip stems` は生成ベースの stems 抽出であり、Suno Web の Pro Get Stems export とは別物です。ユーザーが形式、stems、video を明示しない限り、agent は勝手に切り替えないでください。`--quiet` はダウンロード進捗と通常の状態出力を抑制します。バッチダウンロードが `partial_download` を返した場合は、`error.details.succeeded`、`error.details.failed`、`error.details.not_attempted_clip_ids` を確認し、必要な ID だけを再試行してください。`playlist remove` または複数 clip の publish/reaction 操作が `partial_mutation` を返した場合は、再試行前に `error.details.succeeded_clip_ids`、`error.details.failed`、`error.details.not_attempted_clip_ids` を確認してください。
- Playlist create/set、ローカル画像 upload、clip cover 更新、音声 upload は複数ステップの workflow です。途中の失敗は resource ID、`completed_steps`、`failed.step/code/message`、`recovery` を含む `partial_mutation` を返します。`recovery.resumable=true` の場合だけ構造化された回復コマンドに従い、false の mutation は再実行しないでください。音声 file は stream 送信され、metadata 変更時は要求した field が見えるまで poll します。`clip upload-status` は読み取り専用です。
- ユーザーが明示しない限り、公開、`--captcha` の強制、認証情報の出力、削除系コマンドの実行は行わないでください。削除系コマンドには `-y/--yes` が必須です。
- エラーには推奨アクションが含まれます。

```bash
sunox clip list | jq '.data.clips[0].title'
sunox clip list --liked --public --sort popular --json
sunox agent-info --json
```

セマンティック exit code：

| Code | 意味 | 推奨アクション |
|---|---|---|
| 0 | 成功 | 続行 |
| 1 | 実行時、Web endpoint、部分 mutation、または部分 download エラー | `error.code` と `error.details` を確認してから再試行 |
| 2 | 設定エラー | 設定を修正。盲目的に再試行しない |
| 3 | 認証エラー | `sunox login` を実行 |
| 4 | rate limit | 30-60 秒待って再試行 |
| 5 | 見つからない | ID を確認 |

## Coding Agent Skill としてインストール

```bash
# Codex / Trae CLI
sunox install-skill

# Claude Code
sunox install-skill --target claude

# Cursor
sunox install-skill --target cursor
```

## 実装メモ

生成、description、persona、cover、extend は Suno Web の `/api/generate/v2-web/` を使います。2026-06-30 の HAR で custom create body を再捕捉しました。カスタム歌詞は `gpt_description_prompt` に入り、`prompt` は空のままです。challenge token を送る場合は `token_provider: 1` も送信します。Sunox は現在のアカウントの `/api/billing/info/` `plan.id` から `metadata.user_tier` を埋め、取得できない場合は空値に fallback します。`--enhance-tags` を指定すると、Sunox は先に `/api/prompts/upsample` を呼び、返された tags と `request_id` を `metadata.last_tags_generation` に入れ、`override_fields=["tags"]` を設定します。`personalization_enabled` は捕捉済みの Web submit 形状に合わせます。この flag がない場合、`metadata.last_tags_generation` は送信しません。instrumental create も custom mode を使います。`sunox create --instrumental <prompt>` では prompt を style tags に統合し、送信時の `prompt` は空のままにします。これは `15suno-labs-nostudio-20260630.har` で再捕捉した Web リクエスト形状と一致します。`task: "playlist_condition"` も捕捉済みですが、これは inspiration 生成の別変種で、歌詞は `prompt` に入ります。通常の custom create ルールを流用しないでください。extend は送信前に source clip を読みます。`GET /api/feed/?ids` が source style metadata を返さない場合は、feed/v3 で source title を検索し、完全一致した clip id の metadata だけを merge します。`--title` がなければ `title` に source title を入れ、可能なら source `tags`、`negative_tags`、`metadata.make_instrumental` を継承します。上書きには `--title`、`--tags`、`--exclude`、`--instrumental`、`--no-instrumental` を使います。`clip list` は `POST /api/feed/v3` を使い、`--liked`、`--public`、`--upload`、`--cover`、`--extend`、`--sort popular` などの検索フィルタを提供します。これは library sync ではありません。remaster は捕捉済みの `/api/generate/upsample`、speed adjust は `/api/clips/adjust-speed/` を使います。認証済み生成はデフォルトで challenge token なしで送信します。Suno が required を返し、保存済み Clerk session を refresh できる場合、Sunox は JWT を一度更新して preflight を再実行し、それでも required の場合だけ `--token <solved>` または明示的な `--captcha` を案内します。cover generation と concat edit の body は、まだ新しい live mutation capture が必要です。playlist mutation は bundle/live evidence と endpoint contract test に基づいて実装済みです。`playlist remove` は大きなバッチで Suno 500 が返る場合があるため、clip ごとに 1 リクエストで送信します。

`sunox clip inspire` は live capture 済みの `task=playlist_condition` を実装し、1 つの source clip、tag upsample、`prompt` 内の歌詞だけを扱います。未 capture の複数 source と instrumental variant は公開しません。公開設定の環境変数は `SUNOX_*` を使います。

## コントリビューション

1. ブランチを作成：`git checkout -b feature/your-idea`
2. 変更して `cargo test` を実行
3. PR を作成

`assert_cmd` による統合テスト、OS keychain / Secret Service / CredMan を使った認証情報保存の追加を歓迎します。

## License

MIT。詳細は [LICENSE](LICENSE) を参照してください。
