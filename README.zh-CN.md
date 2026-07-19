# sunox

`sunox` 是一个非官方 Suno 命令行工具，用 Rust 编写。它把网页端常用的创作、下载、
歌单、Persona、翻唱、重制、音频编辑和上传能力带到了终端里。

[![crates.io](https://img.shields.io/crates/v/sunox)](https://crates.io/crates/sunox)
[![CI](https://github.com/ctykwz/sunox/actions/workflows/ci.yml/badge.svg)](https://github.com/ctykwz/sunox/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

[English](README.md) · 简体中文 · [日本語](README.ja.md) · [Français](README.fr.md) ·
[Español](README.es.md)

> [!WARNING]
> Sunox 与 Suno 没有隶属或合作关系，也没有得到 Suno 官方背书。项目调用的是 Suno
> 网页端的非公开接口，接口随时可能调整。请自行遵守 Suno 的服务条款、账号限制，
> 并确认自己有权使用生成或上传的素材。

## 能做什么

- 根据一句描述、自定义歌词、风格标签、Persona 或纯音乐要求创建歌曲。
- 等待异步任务完成，并下载 MP3、M4A、WAV、Opus 或视频。
- 查询、搜索、编辑、公开、删除、恢复和下载歌曲。
- 对已有歌曲做翻唱、续写、拼接、重制、变速、反转、裁剪、淡入淡出或分轨生成。
- 管理歌单和声音 Persona，上传本地音频或封面。
- 在终端里看表格，也可以给脚本或 Coding Agent 输出稳定的 JSON。

Suno Studio 相关能力不在本项目范围内。

## 安装

已经安装 Rust 1.88 或更高版本时，可以直接通过 Cargo 安装：

```bash
cargo install sunox
```

不想安装 Rust，也可以到 [GitHub Releases](https://github.com/ctykwz/sunox/releases)
下载 macOS、Linux 或 Windows 的预编译文件。当前发布文件没有 Apple 或 Windows
商业签名，系统可能会显示常规的下载软件安全提示。每个版本都附带 `SHA256SUMS`，
`sunox update` 更新时会自动校验压缩包。

## 登录

先在本机浏览器登录 suno.com，然后运行：

```bash
sunox login
```

Sunox 会依次查找 Chrome、Edge、Brave、Arc、Chromium 或 Firefox 中可复用的登录状态。
找不到时，才会打开一个独立的浏览器 Profile，让你手动完成登录。

认证信息保存在 Sunox 的本地配置目录中。不要把 Cookie 或 JWT 直接写进命令行、日志、
项目文件或提交记录；无界面服务器请使用 `--cookie-stdin` 或 `--jwt-stdin`。

可以用下面两条命令确认当前状态：

```bash
sunox doctor
sunox credits
```

## 创建并下载一首歌

最简单的方式是直接给一句描述：

```bash
sunox "温暖的氛围电子乐，节奏舒缓，有轻柔的合成器脉冲"
```

需要自定义歌词和生成参数时，使用 `create`：

```bash
sunox create \
  --title "Night Drive" \
  --tags "dream pop, synth, female vocal" \
  --exclude "metal, aggressive" \
  --lyrics-file lyrics.txt \
  --weirdness 35 \
  --style-influence 70
```

一次生成通常会返回两个 Clip ID。先等待生成结束，再下载想保留的版本：

```bash
sunox clip wait <clip_id_1> <clip_id_2>
sunox download <clip_id_1> <clip_id_2> --output ./songs
```

不指定格式时，Sunox 会直接下载现成的 CDN MP3，并把普通歌词和时间轴歌词写入 ID3。
只有明确需要 Suno 的格式转换时，才使用 `--format mp3|m4a|wav|opus`；下载视频则使用
`--video`。

## 常用命令

```text
sunox <描述>                       根据一句描述创建歌曲
sunox create [描述]                使用完整参数创建歌曲
sunox lyrics                       只生成歌词

sunox clip list                    查看自己的歌曲
sunox clip search <关键词>         搜索歌曲
sunox clip info <id>               查看歌曲详情
sunox clip wait <ids>              等待生成完成
sunox download <ids>               下载歌曲

sunox clip cover <id>              翻唱
sunox clip extend <id>             续写
sunox clip concat <ids>            拼接为完整歌曲
sunox clip remaster <id>           重制
sunox clip speed <id>              调整速度
sunox clip reverse <id>            反转音频
sunox clip crop <id>               保留或移除一段音频
sunox clip fade <id>               添加淡入淡出
sunox clip stems <id>              生成分轨

sunox playlist list                查看歌单
sunox playlist create              创建歌单
sunox add <clip_ids> --to <id>     把歌曲加入歌单

sunox persona list                 查看声音 Persona
sunox persona create <clip_id>     从歌曲创建 Persona

sunox clip upload <文件>           上传本地音频
sunox models                       查看账号可用模型
sunox doctor --network             检查 DNS、TCP 和 HTTPS
sunox update                       更新到最新 GitHub Release
```

完整参数以 `sunox --help` 和 `sunox <命令> --help` 为准。

## 生成验证

每次调用生成类接口前，Sunox 都会先执行 Suno 网页端同款验证检查。Suno 没有要求验证时，
请求会直接提交，不会启动浏览器；只有 Suno 明确要求 Challenge 时，Sunox 才会调用匹配的
Chromium 系浏览器完成验证，并在结束后清理临时 Profile。

```text
--captcha          即使预检不要求，也强制执行浏览器验证
--no-captcha       禁止自动调用浏览器验证
--token <token>    使用外部已经解出的 Challenge Token
```

## JSON 与自动化

所有命令都支持 `--json`。stdout 被 Pipe 时也会自动改用 JSON：

```bash
sunox clip list --json
sunox clip list | jq '.data.clips[0].title'
```

错误会返回稳定的错误码和非零退出码。多步骤或批量操作部分失败时，返回值会区分已经完成、
失败和尚未执行的项目，调用方不需要把整批操作重跑一遍。

脚本或 Agent 可以先读取当前版本的机器可读能力：

```bash
sunox agent-info --json
```

也可以安装随项目发布的使用 Skill：

```bash
sunox install-skill                 # Codex
sunox install-skill --target claude
sunox install-skill --target cursor
```

## 配置

```bash
sunox config show
sunox config set output_dir ./songs
sunox config set default_model auto
```

`-c key=value` 只覆盖当前一次调用。环境变量使用 `SUNOX_*` 前缀，例如
`SUNOX_OUTPUT_DIR`、`SUNOX_DEFAULT_MODEL` 和 `SUNOX_BROWSER_PATH`。

同一账号的写操作默认串行执行，避免刷新认证或修改远端资源时互相覆盖。`--parallel` 会为
当前命令关闭这层保护，只应在确定需要并发写入时使用。

部分命令会消耗 Credits 或修改远端资源。新建的歌曲、歌单和 Persona 默认保持私有，只有
显式执行公开命令才会改变可见性；不可恢复的操作必须传入 `-y` 或 `--yes`。

## 开发

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

请从 `main` 新建功能分支，并通过 Pull Request 提交变更。

## 许可证

[MIT](LICENSE)
