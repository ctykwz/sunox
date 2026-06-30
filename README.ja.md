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

## インストール

### Cargo

```bash
cargo install sunox
```

### ビルド済みバイナリ

[GitHub Releases](https://github.com/ctykwz/sunox/releases) から macOS、Linux、Windows 向けバイナリをダウンロードできます。

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
| `-c key=value` / `--config key=value` | その実行だけ設定を上書き。例: `-c default_model=v5.5 -c output_dir=./songs` |
| `-V` / `--version` | バージョンを表示 |
| `-h` / `--help` | コマンドまたはサブコマンドのヘルプ |

## 人向けコマンド

日常利用では主に次の入口だけを使います。

```text
sunox <prompt>                  説明文から直接生成
sunox create [prompt]           タイトル、タグ、歌詞、モデル、persona 付きで生成
sunox download <clip_ids>       完成した曲をダウンロード
sunox add <clip_ids> --to <id>  曲をプレイリストに追加
sunox login                     ブラウザから認証を設定
sunox logout                    ローカル認証情報を削除
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
sunox clip remaster       別モデルで remaster
sunox clip speed          再生速度を調整
sunox clip stems          ボーカルと伴奏の stems を抽出
```

### 参照

```text
sunox clip list
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
sunox download <ids>
sunox clip download <ids>
sunox clip upload <file>
sunox clip delete <ids>
sunox clip restore <ids>
sunox clip like <ids>
sunox clip dislike <ids>
sunox clip set <id>
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

`sunox` は Chrome、Arc、Brave、Firefox、Edge から Clerk cookie を読み取り、JWT に交換し、更新可能なローカル session として保存します。JWT が古くなった場合は、保存済み session から自動更新します。

認証方式：

1. `sunox login`：ブラウザから自動抽出。推奨。
2. `sunox auth --cookie <cookie>`：ヘッドレス環境向けに cookie を手動指定。
3. `sunox auth --jwt <token>`：JWT を直接指定。通常は約 1 時間有効。
4. `sunox auth --refresh`：保存済み Clerk session から JWT を強制更新。

### 生成パラメータ

| Flag | 説明 | 値 |
|---|---|---|
| `--title` | 曲名 | 最大 100 文字 |
| `--tags` | スタイル指定 | 例: `"pop, synths, upbeat"` |
| `--exclude` | 避けたいスタイル | 例: `"metal, heavy, dark"` |
| `--lyrics` / `--lyrics-file` | カスタム歌詞 | `[Verse]` などのセクションタグに対応 |
| `--prompt` | description mode の prompt | 最大 500 文字 |
| `--model` | モデルバージョン | v5.5, v5, v4.5+, v4.5, v4, v3.5, v3, v2 |
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
sunox persona publish <persona_id>
sunox persona unpublish <persona_id>
sunox persona love <persona_id>
sunox persona unlove <persona_id>
sunox persona delete <persona_id> -y
sunox persona restore <persona_id> -y
sunox persona purge <persona_id> -y
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
sunox clip cover <clip_id> --tags "jazz, smooth piano" --model v5.5
sunox clip remaster <clip_id> --model v5.5
sunox clip speed <clip_id> --multiplier 0.94
sunox clip wait <new_clip_id>
sunox download <new_clip_id> --output ./remastered/
```

### 歌詞付きダウンロード

MP3 ダウンロード時に次の ID3 タグを自動で埋め込みます。

- **USLT**：通常歌詞。
- **SYLT**：単語単位の同期歌詞。

```bash
sunox download <id1> <id2> --output ./songs/
sunox download <id1> --video --output ./videos/
```

### 音声アップロード

```bash
sunox clip upload ./demo.mp3 --title "Demo Upload"
sunox clip upload ./demo.wav --lyrics-file lyrics.txt --timeout 900
sunox clip upload ./vocal-stem.wav --stem-mix --title "Vocal stem"
```

## モデル

| Version | Codename | Notes |
|---|---|---|
| **v5.5** | chirp-fenix | デフォルト。最新で最高品質 |
| v5 | chirp-crow | 前世代 |
| v4.5+ | chirp-bluejay | 拡張機能 |
| v4.5 | chirp-auk | 安定版 |
| v4 | chirp-v4 | 旧版 |
| v3.5 | chirp-v3-5 | 旧版 |
| v3 | chirp-v3-0 | 旧版 |
| v2 | chirp-v2-xxl-alpha | 旧版 |

Remaster models: v5.5 = chirp-flounder, v5 = chirp-carp, v4.5+ = chirp-bass。

## Agent 向け出力

- すべてのコマンドが `--json` をサポートします。
- stdout が pipe されると自動で JSON になります。
- 進捗とエラーは stderr に出るため JSON を汚しません。
- エラーには推奨アクションが含まれます。

```bash
sunox clip list | jq '.data[0].title'
sunox agent-info --json
```

セマンティック exit code：

| Code | 意味 | 推奨アクション |
|---|---|---|
| 0 | 成功 | 続行 |
| 1 | 実行時またはネットワークエラー | backoff して再試行 |
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

生成、description、persona、cover、extend は Suno Web の `/api/generate/v2-web/` を使います。2026-06-30 の HAR で custom create body を再捕捉しました。カスタム歌詞は `gpt_description_prompt` に入り、`prompt` は空のままです。challenge token を送る場合は `token_provider: 1` も送信します。`task: "playlist_condition"` も捕捉済みですが、これは inspiration 生成の別変種で、歌詞は `prompt` に入ります。通常の custom create ルールを流用しないでください。remaster は捕捉済みの `/api/generate/upsample`、speed adjust は `/api/clips/adjust-speed/` を使います。認証済み生成はデフォルトで challenge token なしで送信します。Suno が拒否した場合、またはユーザーが明示した場合のみ `--token <solved>` または `--captcha` を使用してください。cover、concat、playlist mutation body はまだ live mutation capture が必要です。

## コントリビューション

1. ブランチを作成：`git checkout -b feature/your-idea`
2. 変更して `cargo test` を実行
3. PR を作成

`assert_cmd` による統合テスト、OS keychain / Secret Service / CredMan を使った認証情報保存の追加を歓迎します。

## License

MIT。詳細は [LICENSE](LICENSE) を参照してください。
