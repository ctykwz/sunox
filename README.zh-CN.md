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

### 纯音乐输入模式

两种模式只能选一种；`--instrumental` 不能和 `--lyrics` 或 `--lyrics-file` 同时使用：

- 只要求无人声、不需要控制内部段落时，单独使用 `--instrumental`。
- 需要控制段落、节奏、剪辑点或配器时，不要传 `--instrumental`，改用结构化歌词文件。
  第一行写 `[Instrumental]`，其余非空行全部放在方括号中，不能留下任何可能被唱出的正文。

```text
[Instrumental]
[Intro — sparse felt piano, free time]
[Build — strings enter and the pulse accelerates]
[Final cut — hard unresolved ending]
```

Clip 完成后，用 `sunox clip timed-lyrics <clip_id> --json` 做人声质量门禁。只要出现一个
`success=true` 且内容非空的对齐词，就淘汰该生成版本。

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
请求会直接提交，不会启动浏览器。Suno 明确要求 Challenge 时，Sunox 会先请求可选的 Browser
Bridge 扩展，在已经打开的 `suno.com` 页面中执行 invisible challenge；如果没有已配对页面响应，
默认的 `auto` 模式才会调用匹配的 Chromium 系浏览器，并在结束后清理临时 Profile。

### 在 macOS 或 Windows 安装 Browser Bridge

Browser Bridge 已经打包在 Sunox 二进制里，不需要另外下载 ZIP，也不需要通过 Chrome
应用商店安装。macOS 和 Windows 的操作完全相同：

1. 运行下面的命令，并记住 Sunox 输出的扩展目录：

```bash
sunox install-browser-extension
```

2. 在平时登录 Suno 的同一个 Chrome Profile 中打开 `chrome://extensions`。
3. 打开右上角的“开发者模式”，选择“加载已解压的扩展程序”，然后选择 Sunox 刚才输出的
   目录。macOS 的 `~/Library` 默认隐藏，需要在文件夹选择器中按 `Shift+Command+G`，再粘贴
   完整路径；Windows 可以把完整路径粘贴到文件夹选择器的地址栏。
4. 保持扩展启用，再打开或刷新一个已经登录的 `https://suno.com` 页面。

扩展安装后，重启 Chrome 不需要重新加载。Sunox 升级并包含新版 Bridge 时，先更新本地文件：

```bash
sunox install-browser-extension --force
```

然后在扩展卡片上点击“重新加载”，并再次刷新 Suno 页面。Sunox 会在 macOS 和 Windows
上自动选择当前用户的应用配置目录；Chrome 使用这个未打包扩展期间，不要移动或删除该目录。

```text
--captcha          即使预检不要求，也强制执行浏览器验证
--no-captcha       禁止自动调用浏览器验证
--token <token>    使用外部已经解出的 Challenge Token
```

`challenge_browser` 支持 `auto`（默认）、`existing`（禁止新开浏览器）和 `isolated`
（始终使用临时浏览器）。单次命令可使用 `-c challenge_browser=existing`。`existing` 模式下，
扩展未连接或版本过旧会直接报错，不会打开其他浏览器；`auto` 模式在没有 Suno 页面响应时
仍可能启动独立浏览器兜底。

无人值守且绝对不能新开窗口时，以“本机已确认安装 Browser Bridge”作为命令选择边界：
已安装就去掉 `--no-captcha`，并使用 `-c challenge_browser=existing`；该模式会自行确认是否有
刷新过的登录态 Suno 页面在线，未连接时直接失败，绝不会新开浏览器。未安装或无法确认是否
安装时，保留 `--no-captcha`，遇到挑战会在提交前停止。仅仅在默认 `auto` 模式下去掉
`--no-captcha`，仍然允许 Sunox 启动独立浏览器兜底。

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
sunox config set challenge_browser auto
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
