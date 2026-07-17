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

Sunox 是非官方项目，与 Suno 不存在隶属或背书关系。项目调用的私有 Web API 可能随时变化；使用者需要自行遵守 Suno 条款、账号限制，以及生成或上传内容涉及的权利要求。

## 安装

### Cargo

```bash
cargo install sunox
```

需要 Rust 1.88 或更高版本。

### 预编译二进制

可以从 [GitHub Releases](https://github.com/ctykwz/sunox/releases) 下载 macOS、Linux 和 Windows 版本。
每个 release 都提供 `SHA256SUMS`；`sunox update` 会在安装前校验所选压缩包。

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
| `--parallel` | 允许本次调用与同账号其他 sunox 进程并行执行 Suno 写操作；默认按账号串行 |
| `-c key=value` / `--config key=value` | 临时覆盖配置，例如 `-c default_model=v5.5 -c output_dir=./songs`，可重复 |
| `-V` / `--version` | 打印版本 |
| `-h` / `--help` | 查看命令或子命令帮助 |

Suno 写操作默认按账号串行。可用 `sunox config set serial_mutations false`
持久关闭，也可用 `-c serial_mutations=false` 单次关闭，或用 `--parallel`
只放开当前命令。
环境变量覆盖统一使用 `SUNOX_*` 前缀，例如 `SUNOX_DEFAULT_MODEL`、`SUNOX_OUTPUT_DIR` 和 `SUNOX_BROWSER_PATH`。

## 人类常用命令

日常使用一般只需要这些入口：

```text
sunox <prompt>                  根据描述直接生成
sunox create [prompt]           用标题、标签、歌词、模型、persona 等参数生成
sunox download <clip_ids>       下载已完成歌曲
sunox add <clip_ids> --to <id>  添加歌曲到歌单
sunox login                     从浏览器配置认证
sunox logout                    删除本地认证和交互登录 profile
sunox doctor                    诊断配置和认证
sunox doctor --network          诊断 DNS、TCP 和 HTTPS（`--strict` 可在异常时返回非零）
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
sunox clip inspire        以一首已有歌曲作为宽松灵感生成新歌
sunox clip remaster       用其他模型重制
sunox clip speed          调整播放速度
sunox clip reverse        反转音频
sunox clip crop           裁剪片段或移除中间片段
sunox clip fade           添加淡入/淡出
sunox clip stems          从已有 clip 生成分轨
```

### 浏览与查看

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

### 管理资源

```text
sunox download <ids>       默认从 CDN 下载 MP3；显式指定 --format mp3|m4a|wav|opus
sunox clip download <ids>  与 download 等价的 agent/高级命令
sunox clip upload <file>
sunox clip upload-status <upload_id>
sunox clip delete <ids> -y
sunox clip restore <ids>
sunox clip purge <ids> -y       # 永久删除垃圾站歌曲，不可恢复
sunox clip empty-trash -y       # 清空垃圾站，不可恢复
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

### 配置与认证

```text
sunox login
sunox logout
sunox auth
sunox config
sunox doctor
sunox doctor --network
sunox agent-info
sunox install-skill
sunox update
```

## 功能

Studio 相关功能不在当前 CLI 的范围内。

### 零摩擦认证

```bash
sunox login
```

`sunox login` 会先从 Chrome、Arc、Brave、Firefox 或 Edge 读取 Clerk cookie；如果读取成功，会记录浏览器来源和可读取的公开 profile 设置，例如接受语言，但不会仅凭浏览器标签伪造 user-agent。如果读取失败，会自动打开一个 Sunox 专用且兼容 Chrome/Edge 的浏览器 profile，等你在里面登录 Suno 后捕获 Clerk session。随后它会换取 JWT，保存可刷新的本地 session，并在 JWT 过期时自动刷新。交互式登录能捕获 user-agent 和接受语言；后续 API 请求会基于选中的 user-agent 派生 Chromium client hints，发送浏览器 fetch metadata header，拿不到真实值时按字段降级。

认证信息以本地 JSON 保存，并未进入系统凭据库。Unix 下认证文件会以 `0600` 创建；Windows 下依赖配置目录的当前用户 ACL。手动传入的 `--cookie` 和 `--jwt` 可能出现在 shell 历史与进程参数中，因此优先使用 `sunox login`，或通过 `--cookie-stdin` / `--jwt-stdin` 从标准输入传入；不要把认证信息写入日志、prompt、项目文件或 commit。

认证方式：

1. `sunox login`：自动浏览器提取，失败后交互式 Chrome/Edge 登录，推荐。
2. `printf '%s' "$SUNOX_COOKIE_INPUT" | sunox auth --cookie-stdin`：从标准输入提供 cookie。
3. `printf '%s' "$SUNOX_JWT_INPUT" | sunox auth --jwt-stdin`：从标准输入提供 JWT。
4. `sunox auth --refresh`：从已保存 Clerk session 强制刷新 JWT。

`sunox logout` 会删除本地认证、交互式登录 profile 和旧版 captcha profile。

### 生成参数

| 参数 | 作用 | 取值 |
|---|---|---|
| `--title` | 歌曲标题 | 最多 100 字符 |
| `--tags` | 风格方向 | 按模型和账号限制；用 `sunox models --json` 查看 |
| `--enhance-tags` | 提交前用 Suno Web 的 tag upsample 增强风格标签 | 显式开启 |
| `--exclude` | 排除风格 | 按模型和账号限制；用 `sunox models --json` 查看 |
| `--lyrics` / `--lyrics-file` | 自定义歌词 | 对应 `max_lengths.gpt_description_prompt` |
| `--prompt` | 描述模式 prompt | 对应 `max_lengths.prompt` |
| `--model` | 模型版本 | v5.5, v5, v4.5+, v4.5-all, v4.5, v4, v3.5, v3, v2 |
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
sunox persona publish <persona_id>        # 仅在明确要公开时使用
sunox persona unpublish <persona_id>
sunox persona love <persona_id>
sunox persona unlove <persona_id>
sunox persona delete <persona_id> -y
sunox persona restore <persona_id>
sunox persona purge <persona_id> -y       # 彻底删除
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
# 以下命令可能返回 submitted/processing clip；下游操作前先 wait
sunox clip cover <clip_id> --tags "jazz, smooth piano" --model v5.5
sunox clip inspire <clip_id> --title "新歌" --tags "garage pop" --lyrics-file lyrics.txt
sunox clip remaster <clip_id> --model v5.5 --variation subtle # subtle、normal 或 high
sunox clip speed <clip_id> --multiplier 0.94
sunox clip reverse <clip_id>
sunox clip wait <new_clip_id>
sunox download <new_clip_id> --output ./remastered/

# crop/fade 成功返回时结果 clip 已 complete，无需再次 wait
sunox clip crop <clip_id> --start 12.5 --end 74.0
sunox clip crop <clip_id> --start 30.0 --end 45.0 --remove-section
sunox clip fade <clip_id> --in 2.0 --out 78.5
```

### 下载并嵌入歌词

下载 MP3 时会自动写入：

- **USLT**：普通歌词。
- **SYLT**：逐词同步歌词。

```bash
sunox download <id1> <id2> --output ./songs/

# 已有同名文件时，只有显式要求覆盖才使用 --force
sunox download <id1> --output ./songs/ --force
sunox download <id1> --format wav --output ./songs/
sunox download <id1> --video --output ./videos/
```

文件名格式为 `title-slug-clipid8.<ext>`。输出目录会自动创建；已有文件默认保留，只有显式传入 `--force` 才会覆盖。

### 上传音频

```bash
sunox clip upload ./demo.mp3 --title "Demo Upload"
sunox clip upload ./demo.wav --lyrics-file lyrics.txt --timeout 900
sunox clip upload ./vocal-stem.wav --stem-mix --title "Vocal stem"
sunox clip upload-status <upload_id> --json  # 只读状态，不重放上传写操作
```

## 模型

| 版本 | Codename | 说明 |
|---|---|---|
| auto | 账号响应 | CLI 默认，选择当前账号可用的默认模型 |
| v5.5 | chirp-fenix | 最新一代；仅在 billing 读取失败时 fallback |
| v5 | chirp-crow | 上一代 |
| v4.5+ | chirp-bluejay | 扩展能力 |
| v4.5-all | chirp-auk-turbo | 账号提供时可作为免费档模型 |
| v4.5 | chirp-auk | 稳定版 |
| v4 | chirp-v4 | 旧版 |
| v3.5 | chirp-v3-5 | 旧版 |
| v3 | chirp-v3-0 | 旧版 |
| v2 | chirp-v2-xxl-alpha | 旧版 |

Remaster 模型：v5.5 = chirp-flounder，v5 = chirp-carp，v4.5+ = chirp-bass。

模型可用性、账号默认模型和长度限制都由账号决定。默认 `default_model=auto` 会直接从 `/api/billing/info/` 选择当前账号可用的默认模型；`sunox models --json` 用于查看同一份账号数据。显式模型会在 billing 信息可用时校验 `can_use` 和 `max_lengths`，只有 billing 读取失败时才 fallback 到 v5.5。

## Agent 友好输出

- 每个命令都支持 `--json`。
- stdout 被 pipe 时会自动切到 JSON。
- 进度和错误写 stderr，不污染 JSON。
- Suno 写操作默认按账号串行；除非用户明确允许同账号并发写入，否则不要使用 `sunox config set serial_mutations false`、`-c serial_mutations=false` 或 `--parallel`。
- 普通音频检查优先使用现有 clip 媒体：`sunox clip info <id> --json` 会给出 `audio_url`，并额外包含 `attribution`、`comments`、`direct_children_count` 和 `similar_clips`；如果非认证、非限流的补充读接口失败，基础 clip 仍会返回，并带上 `supplemental_errors`，认证和限流错误仍会正常中断。`sunox clip download` 默认直接下载该 `audio_url` 的 CDN MP3 并写入歌词；显式 `--format mp3|m4a|wav|opus` 才请求 Suno 官方对应格式，`--video` 只在有 `clip.video_url` 时使用。`sunox clip stems` 是生成式分轨，不等同于 Suno Web 的 Pro Get Stems 导出。除非用户明确要求格式、stems 或 video，否则 agent 不应主动切换。`--quiet` 会抑制下载进度和普通状态输出。若批量下载返回 `partial_download`，重试前查看 `error.details.succeeded`、`error.details.failed` 和 `error.details.not_attempted_clip_ids`，只重试必要 ID。若 `playlist remove` 或多首歌曲的发布/反应命令返回 `partial_mutation`，重试前先查看 `error.details.succeeded_clip_ids`、`error.details.failed` 和 `error.details.not_attempted_clip_ids`。
- Playlist create/set、本地图片上传、歌曲封面更新和音频上传属于多步骤工作流。服务端已有步骤完成后若后续失败，会返回包含资源 ID、`completed_steps`、`failed.step/code/message` 和 `recovery` 的 `partial_mutation`。只有 `recovery.resumable=true` 时才执行其中的结构化恢复命令，禁止重放标记为 false 的 mutation。音频文件会流式发送到预签名地址；若修改了 metadata，会轮询到目标字段可见。`clip upload-status` 只读状态，不会续跑或重放写操作。
- 除非用户明确要求，不要发布/公开资源、不要强制 `--captcha`、不要输出认证材料，也不要运行删除/清理类命令；这类命令必须带 `-y/--yes`。
- 错误响应包含建议动作。

```bash
sunox clip list | jq '.data.clips[0].title'
sunox clip list --liked --public --sort popular --json
sunox agent-info --json
```

语义化 exit code：

| Code | 含义 | 建议动作 |
|---|---|---|
| 0 | 成功 | 继续 |
| 1 | 运行时、Web 接口、部分写入或部分下载失败 | 先查看 `error.code` 和 `error.details` 再决定是否重试 |
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

生成、描述、persona、cover、extend 等路径复用 Suno Web 的 `/api/generate/v2-web/`。2026-06-30 的 HAR 已重新捕获 custom create body：自定义歌词放在 `gpt_description_prompt`，`prompt` 保持为空；带 challenge token 时同时发送 `token_provider: 1`。Sunox 会优先从当前账号 `/api/billing/info/` 的 `plan.id` 填充 `metadata.user_tier`，拿不到时降级为空值。使用 `--enhance-tags` 时，Sunox 会先调用 `/api/prompts/upsample`，再把返回的 tags 和 `request_id` 写入 `metadata.last_tags_generation`，并设置 `override_fields=["tags"]`；其中 `personalization_enabled` 按已捕获的 Web submit 形状发送。不使用该参数时不会发送 `metadata.last_tags_generation`。纯音乐 create 也走 custom mode；`sunox create --instrumental <prompt>` 会把 prompt 合并进 style tags，提交时 `prompt` 字段仍保持为空，这与 `15suno-labs-nostudio-20260630.har` 中重新捕获的 Web 请求一致。`task: "playlist_condition"` 也已捕获，但它属于 inspiration 生成变体，歌词放在 `prompt`，不能套用普通 custom create 规则。extend 会在提交前读取源 clip；当 `GET /api/feed/?ids` 缺少源 style metadata 时，会通过 feed/v3 按源标题搜索并用精确 clip id 合并 metadata；默认把 `title` 设为源标题，尽量继承源 `tags`、`negative_tags` 和 `metadata.make_instrumental`；需要覆盖时可用 `--title`、`--tags`、`--exclude`、`--instrumental` 或 `--no-instrumental`。`clip list` 使用 `POST /api/feed/v3`，支持 `--liked`、`--public`、`--upload`、`--cover`、`--extend`、`--sort popular` 等查询过滤；这不是 library sync。remaster 使用已捕获的 `/api/generate/upsample`，speed adjust 使用 `/api/clips/adjust-speed/`。默认提交不携带 challenge token；如果 Suno 报 required 且本地有 Clerk refresh material，Sunox 会先刷新一次 JWT 并重新 preflight，仍然 required 时才提示使用 `--token <solved>` 或显式 `--captcha`。cover 生成和 concat 编辑的 body 仍需要新的 live mutation capture。playlist mutation 已基于 bundle/live evidence 和 endpoint contract tests 实现；playlist remove 因大批量 remove 线上可能返回 Suno 500，故按 clip 单个请求提交。

`sunox clip inspire` 已实现 live-captured `task=playlist_condition`：仅接受一个来源 clip，先执行真实 tag upsample，把歌词放入 `prompt`，并携带返回的 `request_id`。未捕获的多来源和纯音乐变体不对外暴露。公开配置环境变量统一使用 `SUNOX_*` 前缀。

## 贡献

1. 创建分支：`git checkout -b feature/your-idea`
2. 修改并运行 `cargo test`
3. 提交 PR

欢迎补充 `assert_cmd` 集成测试，以及 OS keychain / Secret Service / CredMan 认证存储支持。

## License

MIT，见 [LICENSE](LICENSE)。
