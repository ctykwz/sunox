<div align="center">

# sunox

**在终端里生成 AI 音乐，直接复用 Suno Web 工作流**

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

`sunox` 是一个单文件 Rust CLI，直接调用 Suno Web 端点。它支持自定义歌词、风格标签、个人声音 persona、人声控制、weirdness/style 滑杆、翻唱、重制、速度调整、分轨提取，以及下载时自动写入歌词标签。

**语言:** [English](README.md) | 简体中文 | [日本語](README.ja.md) | [Français](README.fr.md) | [Español](README.es.md)

[安装](#安装) | [快速开始](#快速开始) | [人类常用命令](#人类常用命令) | [Agent 与高级命令](#agent-与高级命令) | [功能](#功能) | [贡献](#贡献)

</div>

## 为什么做这个

Suno 的 Web UI 适合手动操作，但不适合脚本化、从文件读取歌词、批量生成，或者接入终端音乐工作流。

`sunox` 解决这些问题：自动从浏览器提取认证、把核心生成参数暴露成 CLI flag、同时支持人类可读输出和 JSON 输出，并在下载 MP3 时自动嵌入同步歌词。

## 安装

### Cargo

```bash
cargo install sunox
```

### 预编译二进制

可以从 [GitHub Releases](https://github.com/ctykwz/sunox/releases) 下载 macOS、Linux 和 Windows 版本。

### 自更新

```bash
sunox update --check    # 查看是否有新版本
sunox update            # 安装最新 release
```

当 Suno 修改 Web schema 时，优先运行 `sunox update`，通常比等待包管理器更新更快。

## 快速开始

```bash
# 1. 登录，自动从 Chrome / Arc / Brave / Firefox / Edge 提取认证
sunox login

# 2. 直接用自然语言生成
sunox "一首下雨早晨的 chill lo-fi"

# 3. 使用完整控制参数生成
sunox create \
  --title "Weekend Code" \
  --tags "indie rock, guitar, upbeat" \
  --exclude "metal, heavy" \
  --lyrics-file lyrics.txt \
  --vocal male \
  --weirdness 40 \
  --style-influence 65

# 4. 等待返回的 clip ID 完成，然后下载
sunox clip wait <clip_id_1> <clip_id_2>
sunox download <clip_id_1> <clip_id_2> --output ./songs/

# 5. 添加到歌单
sunox add <clip_id> --to <playlist_id>
```

Agent 或脚本应先运行 `sunox agent-info --json` 获取机器可读能力，再用 `--json` 调用资源命令。

## 全局选项

| 选项 | 说明 |
|---|---|
| `--json` | 强制输出结构化 JSON；stdout 被 pipe 时会自动启用 |
| `--quiet` | 减少非必要进度输出 |
| `-c key=value` / `--config key=value` | 临时覆盖配置，例如 `-c default_model=v5.5 -c output_dir=./songs`，可重复 |
| `-V` / `--version` | 打印版本 |
| `-h` / `--help` | 查看命令或子命令帮助 |

## 人类常用命令

日常使用一般只需要这些入口：

```text
sunox <prompt>                  根据描述直接生成
sunox create [prompt]           用标题、标签、歌词、模型、persona 等参数生成
sunox download <clip_ids>       下载已完成歌曲
sunox add <clip_ids> --to <id>  添加歌曲到歌单
sunox login                     从浏览器配置认证
sunox logout                    删除本地认证
sunox doctor                    诊断配置和认证
```

## Agent 与高级命令

`sunox` 保留完整 Suno 资源命令，供 Codex 风格 agent、自动化脚本和调试使用。Agent 应优先使用 `--json`，并通过 `sunox agent-info --json` 获取当前契约。

### 创建与变换

```text
sunox create              描述模式或自定义歌词模式
sunox lyrics              只生成歌词，不消耗 credits
sunox clip extend         从某个时间点继续生成
sunox clip concat         拼接 clip 成完整歌曲
sunox clip cover          用新风格或模型做 cover
sunox clip remaster       用其他模型重制
sunox clip speed          调整播放速度
sunox clip stems          提取人声和伴奏分轨
```

### 浏览与查看

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

### 管理资源

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

### 配置与认证

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

## 功能

### 零摩擦认证

```bash
sunox login
```

`sunox` 会从 Chrome、Arc、Brave、Firefox 或 Edge 读取 Clerk cookie，换取 JWT，保存可刷新的本地 session，并在 JWT 过期时自动刷新。

认证方式：

1. `sunox login`：自动浏览器提取，推荐。
2. `sunox auth --cookie <cookie>`：在无头服务器上手动粘贴 cookie。
3. `sunox auth --jwt <token>`：直接提供 JWT，通常约 1 小时有效。
4. `sunox auth --refresh`：从已保存 Clerk session 强制刷新 JWT。

### 生成参数

| 参数 | 作用 | 取值 |
|---|---|---|
| `--title` | 歌曲标题 | 最多 100 字符 |
| `--tags` | 风格方向 | 例如 `"pop, synths, upbeat"` |
| `--exclude` | 排除风格 | 例如 `"metal, heavy, dark"` |
| `--lyrics` / `--lyrics-file` | 自定义歌词 | 支持 `[Verse]` 等段落标签 |
| `--prompt` | 描述模式 prompt | 最多 500 字符 |
| `--model` | 模型版本 | v5.5, v5, v4.5+, v4.5, v4, v3.5, v3, v2 |
| `--vocal` | 人声性别 | male, female |
| `--persona` | 声音 persona ID | Suno 里的声音 UUID |
| `--weirdness` | 实验程度 | 0-100 |
| `--style-influence` | 风格遵循程度 | 0-100 |
| `--instrumental` | 纯音乐 | flag |

### Voice Personas

```bash
sunox persona list
sunox persona info <persona_id>
sunox persona create <clip_id> --name "My Voice" --description "Warm lead vocal"
sunox create --persona <persona_id> --title "My Song" --tags "pop" --lyrics "[Verse]\nHello world"
```

也可以发布、取消发布、收藏、删除、恢复或彻底删除 persona：

```bash
sunox persona publish <persona_id>
sunox persona unpublish <persona_id>
sunox persona love <persona_id>
sunox persona unlove <persona_id>
sunox persona delete <persona_id> -y
sunox persona restore <persona_id> -y
sunox persona purge <persona_id> -y
```

### 歌单

```bash
sunox playlist list
sunox playlist create --name "Release candidates" --description "Tracks to review"
sunox add <clip_id_1> <clip_id_2> --to <playlist_id>
sunox playlist remove <playlist_id> <clip_id_1>
sunox playlist publish <playlist_id> --private
sunox playlist reorder <playlist_id> --clip-id <clip_id> --index 0
```

### Clip 变换

```bash
sunox clip cover <clip_id> --tags "jazz, smooth piano" --model v5.5
sunox clip remaster <clip_id> --model v5.5
sunox clip speed <clip_id> --multiplier 0.94
sunox clip wait <new_clip_id>
sunox download <new_clip_id> --output ./remastered/
```

### 下载并嵌入歌词

下载 MP3 时会自动写入：

- **USLT**：普通歌词。
- **SYLT**：逐词同步歌词。

```bash
sunox download <id1> <id2> --output ./songs/
sunox download <id1> --video --output ./videos/
```

### 上传音频

```bash
sunox clip upload ./demo.mp3 --title "Demo Upload"
sunox clip upload ./demo.wav --lyrics-file lyrics.txt --timeout 900
sunox clip upload ./vocal-stem.wav --stem-mix --title "Vocal stem"
```

## 模型

| 版本 | Codename | 说明 |
|---|---|---|
| **v5.5** | chirp-fenix | 默认，最新质量最好 |
| v5 | chirp-crow | 上一代 |
| v4.5+ | chirp-bluejay | 扩展能力 |
| v4.5 | chirp-auk | 稳定版 |
| v4 | chirp-v4 | 旧版 |
| v3.5 | chirp-v3-5 | 旧版 |
| v3 | chirp-v3-0 | 旧版 |
| v2 | chirp-v2-xxl-alpha | 旧版 |

Remaster 模型：v5.5 = chirp-flounder，v5 = chirp-carp，v4.5+ = chirp-bass。

## Agent 友好输出

- 每个命令都支持 `--json`。
- stdout 被 pipe 时会自动切到 JSON。
- 进度和错误写 stderr，不污染 JSON。
- 错误响应包含建议动作。

```bash
sunox clip list | jq '.data[0].title'
sunox agent-info --json
```

语义化 exit code：

| Code | 含义 | 建议动作 |
|---|---|---|
| 0 | 成功 | 继续 |
| 1 | 运行时错误或网络错误 | 退避重试 |
| 2 | 配置错误 | 修复配置，不要盲目重试 |
| 3 | 认证错误 | 运行 `sunox login` |
| 4 | 限流 | 等 30-60 秒后重试 |
| 5 | 资源不存在 | 检查 ID |

## 安装为 Coding Agent Skill

```bash
# Codex / Trae CLI
sunox install-skill

# Claude Code
sunox install-skill --target claude

# Cursor
sunox install-skill --target cursor
```

## 实现说明

生成、描述、persona、cover、extend 等路径复用 Suno Web 的 `/api/generate/v2-web/`。2026-06-30 的 HAR 已重新捕获 custom create body：自定义歌词放在 `gpt_description_prompt`，`prompt` 保持为空；带 challenge token 时同时发送 `token_provider: 1`。`task: "playlist_condition"` 也已捕获，但它属于 inspiration 生成变体，歌词放在 `prompt`，不能套用普通 custom create 规则。remaster 使用已捕获的 `/api/generate/upsample`，speed adjust 使用 `/api/clips/adjust-speed/`。默认提交不携带 challenge token；只有 Suno 拒绝请求或用户明确要求时才使用 `--token <solved>` 或 `--captcha`。cover、concat 和 playlist mutation body 仍需要 live mutation capture。

## 贡献

1. 创建分支：`git checkout -b feature/your-idea`
2. 修改并运行 `cargo test`
3. 提交 PR

欢迎补充 `assert_cmd` 集成测试，以及 OS keychain / Secret Service / CredMan 认证存储支持。

## License

MIT，见 [LICENSE](LICENSE)。
