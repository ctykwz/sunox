# sunox

`sunox` 是一个非官方 Suno 命令行工具，用 Rust 编写。它把网页端常用的创作、下载、
歌单、Persona/Voice、Custom Model、分轨、封面媒体、音频编辑和上传能力带到了终端里。

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
- 对已有歌曲做翻唱、续写、拼接、重制、变速、反转、裁剪、淡入淡出或 Pro 分轨。
- 读取/下载已有分轨结果，管理歌词项目和 Custom Model，并从本地录音创建私有验证 Voice。
- 生成并应用封面图，并查询已有歌曲视频状态；旧视频提交在 Clip 资格可证明前保持 fail-closed。
- 管理歌单和 Persona，上传本地音频或封面。
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
  --duration 180 \
  --weirdness 35 \
  --style-influence 70 \
  --variety 3
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

不指定格式时，Sunox 会通过 Suno 官方 prepared-download 接口下载 MP3，并把普通歌词和
时间轴歌词写入 ID3。用 `--format mp3|m4a|wav|opus` 选择其他格式；WAV/OPUS 会先读取已有
转换结果，缺失时才发起服务端转换。传 `--no-convert` 可禁止这个 POST；下载视频使用
`--video`。prepared download 即使是 GET，也可能计入套餐下载额度。

下载内容必须非空且通过基本媒体容器检查，才会替换目标文件。HTTP 200 错误页或截断的文件头
会导致下载失败，即使使用 `--force` 也会保留原文件。Opus 会检查全部 Ogg 页，并要求完整的
头部和音频包；WAV 会核对 RIFF/RF64 声明的区块长度及 PCM 帧对齐。这些检查不包含完整媒体解码。

## 常用命令

```text
sunox <描述>                       根据一句描述创建歌曲
sunox create [描述]                使用完整参数创建歌曲
sunox lyrics                       只生成歌词
sunox lyrics rewrite --prompt "..." --edit-file selection.txt
                                     重写一段选中的歌词
sunox lyrics mashup --lyrics-a-file a.txt --lyrics-b-file b.txt
                                     启动双源歌词混合
sunox lyrics mashup-status <id> --wait
                                     只读等待已有混合任务完成

sunox clip list                    查看自己的歌曲
sunox clip search <关键词>         搜索歌曲
sunox clip info <id>               查看歌曲详情
sunox clip actions <id>            查看服务端对该歌曲开放的操作
sunox clip wait <ids>              等待生成完成
sunox download <ids>               下载歌曲

sunox clip cover <id>              翻唱
sunox clip reuse <id>              复用源歌曲的歌词和风格
sunox clip underpaint <id>         为自有的人声/上传音频添加伴奏
sunox clip overpaint <id>          为自有的纯音乐/上传音频添加人声
sunox clip extend <id>             续写
sunox clip concat <ids>            拼接为完整歌曲
sunox clip remaster <id>           重制
sunox clip speed <id>              调整速度
sunox clip reverse <id>            反转音频
sunox clip crop <id>               保留或移除一段音频
sunox clip fade <id>               添加淡入淡出
sunox clip stems <id>              Pro Auto Split（当前 50 credits）
sunox clip stems <id> --mode split --stem vocals
                                     Pro Split from Mix（当前一对 20 credits）
sunox clip get-stems <id>          只读已有分轨结果，不启动拆分
sunox clip generate-image <id> --prompt "..."
                                     生成并应用封面图
sunox clip generate-video <id>     校验旧视频资格（当前提交 fail-closed）
sunox clip video-status <id>       只读视频任务状态
sunox clip cover-art models        查看当前图片/视频模型类别与允许时长
sunox clip cover-art image <id> --prompt "..."
                                     生成两个图片候选，不自动应用
sunox clip cover-art video <id> --prompt "..."
                                     生成两个视频候选，不自动应用
sunox clip cover-art status <batch_id> --media image --wait
sunox clip cover-art apply-image <id> <batch_id> <image_id>
sunox clip cover-art apply-video <id> <batch_id> <video_upload_id>

sunox playlist list                查看歌单
sunox playlist create              创建歌单
sunox add <clip_ids> --to <id>     把歌曲加入歌单

sunox persona list                 查看声音 Persona
sunox persona create <clip_id>     从歌曲创建 Persona
sunox voice phrase --language zh   获取当前验证短语
sunox voice create --help          用两个 WAV 文件创建私有验证 Voice

sunox models custom pending        查看训练中的 Custom Model
sunox models custom train --help   查看权利与 Web UI 显式确认要求
sunox models custom archive <id> -y
                                     归档 Custom Model

sunox lyrics projects list         查看歌词项目
sunox lyrics projects info <id>    精确读取一个歌词项目
sunox lyrics projects create       创建歌词项目
sunox lyrics projects rename <id> --title "..."
                                     重命名并读回歌词项目
sunox lyrics projects flush <id> --lyrics-file lyrics.txt
                                     立即保存歌词项目
sunox lyrics projects delete <id> -y
                                     显式确认后删除歌词项目
sunox create --lyrics-file lyrics.txt --lyrics-project-id <id>
                                     将精确歌词项目关联到自定义歌词生成

sunox clip upload <文件>           上传本地音频
sunox models                       查看账号可用模型
sunox capabilities                 查看账号权益与 CLI 适配矩阵
sunox doctor --network             检查 DNS、TCP 和 HTTPS
sunox update                       更新到最新 GitHub Release
```

完整参数以 `sunox --help` 和 `sunox <命令> --help` 为准。

### 这次补齐的 Pro 能力边界

`clip stems` 会启动计费的 `gen_stem` 任务；`clip get-stems` 只读取已有结果页，只有显式
`--download` 才下载。Pro 支持 Auto Split 和 12 个规范目标的 Split from Mix；Premier 专属的
任意 Advanced Split 乐器仍然不开放，避免按未确认映射扣费。只要分页结果中有任何 stem ID 无法
补全，下载就会 fail-closed。MP3 分轨下载不会额外请求时间轴歌词；WAV/OPUS 在未传 `--no-convert`
且非全局 `--read-only` 时仍可能发起转换，prepared download 也可能消耗套餐下载额度。

创建 Voice 前先用 `voice phrase` 获取动态短语。把演唱样本和该短语录音准备成 WAV 后，使用
`voice create --confirm-rights --confirm-eligibility --confirm-biometric-consent`。三项分别确认录音
权利、当前 18+/地区/音频上传资格，以及 Suno 对录音可能构成生物识别数据的收集处理；Suno 条款、
隐私政策、训练用途/账号选择和服务端门禁仍是最终依据。CLI 会完成两次上传、处理、所有权验证、
私有 Vox Persona 创建，并读回确认 `is_public=false` 和 Vox 类型；CLI 本身不直接录制麦克风。
每个服务端 ID 都会原子写入 Sunox 管理配置目录下的 checkpoint，供中断后检查，但不代表多写流程
可以安全续跑。Web 会先裁切演唱样本再上传，因此 Sunox 只接受已经预裁切、实测 WAV 时长与
`--sample-duration` 精确一致的样本，绝不会静默多上传音频。当前 Web 规则是源文件不足 10 秒时
整段使用，否则选择 10 到 240 秒；验证录音仍以服务端为准，Web 当前目标约 15 秒，通用上传上限
为 900 秒。后续用 Persona ID 配合 `create --persona`；所选实时账号模型需要声明当前 Vox
能力，v6 模型已支持。

Lyrics 2.0 的选区重写、双源 mashup 轮询、歌词项目 CRUD/flush，以及
`create --lyrics-project-id` 精确关联分别使用各自当前路由。rewrite 是单次 30 秒同步请求，mashup
默认等待且轮询有明确上限；`--no-wait` 会返回 ID，供只读 `mashup-status` 查询，后者的
`--timeout` 必须和 `--wait` 一起使用。传输结果不确定时都不会自动重放，歌词项目删除必须显式
传 `-y/--yes`。音频 underpaint/overpaint 是独立的生成型 clip 命令；Song Editor 区段替换不属于
这些歌词命令，也没有被冒充为已支持。

`clip reuse` 会精确读取源 clip，并且只在对应 CLI 参数缺省时填入歌词、风格、排除风格和标题；
显式的 `--lyrics`/`--lyrics-file`、`--tags`、`--exclude`、`--title` 优先。CLI 会校验实时模型的
`reuse_styles_lyrics` feature，但不会把这个 UI condition 当成 generation task 发送。
`clip underpaint` / `clip overpaint` 使用当前的 `underpainting` / `overpainting` task 和对应源 ID
字段。提交前会 fail-closed 校验 Edit Mode 权限、clip 完成且明确未删除、JWT 所属账号与 clip owner
一致、源音频满足当前 Web 的人声/纯音乐 eligibility，以及实时模型支持对应的
`underpaint` / `overpaint` condition。这些操作可能消耗 credits，最终资格与计费仍由服务端决定。

Custom Model 训练至少需要 6 个不同源 Clip ID、`--confirm-rights`、账号实时可见的
`custom_models` entitlement，以及 `--confirm-ui-available`：后者只能在当前 Suno Web 账号确实能看到
训练 UI 后显式传入。缺少该 UI 确认时，CLI 不会发送训练 POST。当前 Web 显示 100 credits，最终资格与
计费仍以服务端为准。pending 查询、精确 ID archive 和已就绪模型选择也可用；已就绪模型会出现在
`sunox models`，可传给 `create --model`。archive 不代表永久删除或承诺可恢复。

`clip generate-image` 实现直接的 `prompt_image` 加 `set_metadata` 组合；独立的 `clip cover-art`
命名空间实现新版多结果 `SONG_COVER_ART` 图片/视频工作流，包括动态模型/时长、费用预检、
pending/history 恢复、bounded polling 和显式 apply。生成不会自动应用第一个结果。两条路径都要求
JWT 账号与 Clip owner 精确一致、明确未删除，并且 `generate_cover_art` action 可用。
旧逐 Clip 视频 POST/status 是另一套独立协议；路由虽已确认，
但精确的 ownership/download eligibility seam 尚未确认，因此 `clip generate-video` 会在 POST 前
fail-closed；`clip video-status` 保留为 bounded 只读状态查询。

Cowrite 歌词模型会在运行时从 Suno 查询。可用
`sunox lyrics --prompt "..." --model <模型>` 按 ID 或展示名选择模型；只有模型声明支持时
才使用 `--thinking`。`clip inspire` 可通过 `--audio-influence 0..100` 设置当前 Web
协议中的 `audio_weight`。

普通生成和 Cover 也不再依赖 CLI 内置的固定模型枚举：`--model` 可传当前账号模型的展示名、
external key 或账号 model ID；不可用或同名歧义会在提交前失败。新安装默认使用 v6 Pro
`chirp-hawk`；可通过 `--model`、`SUNOX_DEFAULT_MODEL` 或 `config set default_model` 覆盖。
v6 Custom 生成的 `--duration <秒>` 支持 10 到 360 的整数秒；若账号模型返回了更严格的 duration
上限，CLI 也会按该值校验。精确的 v5.5 `chirp-fenix` 仍保留兼容 duration 路径；v6 描述模式
不开放 duration；v6 Custom 未指定时长时使用当前 Web 默认值 180 秒。v6 Custom 还支持
整数 `--variety 0..4`、由模型与 session gate 共同控制的 `--mumble` 非词汇
人声，以及由账号权益、session gate 和模型共同控制的 `--max-mode`；实时能力未声明支持时会在提交前失败。
Variety 还要求当前 Web 的 `aug-creativity` flag；该 gate 可用且未传 `--variety` 时，Sunox 跟随
Web 默认值：`chirp-hawk-wild` 为 0，其他 v6 模型为 1；gate 缺失时省略默认值。
提交后仍以 Suno 服务端为准。Sunox 会在这些受限控制写入前读取 `/api/session/`，按需要求
`aug-creativity`、`mumble-mode` 或 `max-mode` flag；当前账号缺少 Mumble flag，因此会在提交前明确拒绝。Max Mode
可以端到端保留，但不严格遵守短 `--duration`；启用 `--max-mode` 时应把时长视为请求值，而不是
对最终成品长度的保证。
`sunox capabilities --json` 会同时展示当前套餐、实时模型选择器、账号限制和各项权益的 CLI
覆盖状态。

Remaster 除了校验账号 feature 和模型列表，还会在提交前读取源 Clip：必须精确命中、已完成、
未进回收站、不是 infill、时长不超过 960 秒，并且 `action_config` 中 Remaster 为
`visible=true, disabled=false`。v4.5+ `chirp-bass` 协议不发送 `variation_category`，因此该模型
显式传 `--variation` 会被本地拒绝。未传 `--model` 时优先选择实时列表中标记为默认且 CLI 已知
请求形状的模型，再回退到第一个已支持模型；如果账号只暴露未知的未来模型，会在提交前失败。已支持的模型可按
`capabilities` 返回的展示名或 external key 传给 `--model`。v6 Remaster 使用
`chirp-halibut`。variation 支持 `subtle`、默认 `normal`、`high`；style profile 支持
`natural`、默认 `boost`、`clarity`，当前 Web 会同时发送两个默认值。真实 v6 Remaster 使用
`--variation high --style-profile clarity` 已完成并在最终元数据中保留这两个值。虽然底层 Web
请求 builder 仍包含 tags 和五个旧 slider 字段，但当前 v2 modal 不展示它们：tags 被服务端明确
拒绝为 staff-only，freedom 被拒绝，其他 slider 即使接受也没有最终元数据回显。Sunox 因此不公开
这些尚未证明生效的参数，避免用户为隐藏实验字段消耗 credits。

## 生成验证

每次调用生成类接口前，Sunox 都会先执行 Suno 网页端同款验证检查。不需要验证时，请求会直接
提交，不会启动浏览器。Suno 要求 Challenge 时，Sunox 会优先请求可选的 Browser Bridge 扩展，
在用户日常使用的 Chrome Profile 中执行不可见的验证组件。扩展空闲时只保留本地监听器；需要
验证时，它会在 Chrome 不可见的 offscreen document 中创建一个绑定随机 nonce 的 `suno.com`
iframe。iframe 保留正常布局尺寸，供对可见性敏感的验证代码计算，但 Chrome 不会创建标签页、
弹窗、最小化窗口或独立浏览器进程。该 iframe 使用当前 Chrome Profile 的 Suno 上下文，让验证
服务看到与 Suno Web 一致的浏览器状态。Bridge 会在 `document_start` 阶段、宿主脚本运行前停止
宿主响应，并将其替换为只允许当前验证服务运行的最小文档；响应规则会移除 `Set-Cookie` 等存储
副作用，同时移除 `Location` 等导航副作用。规则会在导航前安装，固定的
`https://suno.com/` 根地址只用来承载同源文档；Bridge 不再探测或跟随应用路由。标准 3xx 响应
在 `Location` 已被成功移除时仍可作为受控文档使用；Chrome 一旦真的开始跳转，就立即失败退出。
只有扩展拥有的一级 iframe，且一次性承载请求标记、文档 nonce、请求头和受控响应头全部一致时
才能连接。异常跳转、缓存命中、重载、断连、协议漂移或身份不匹配都会删除 iframe 并失败退出；
得到 Token 或终态错误后也会立即删除 iframe，且绝不降级到任何可见窗口或独立浏览器。页面原始
错误不会越过 Bridge 边界，也不会写入存储或日志。该流程同时支持 macOS 和 Windows。
如果 Turnstile 第一次静默执行 15 秒后完全没有回调，Bridge 会删除旧组件，并且只重建一次
全新的静默组件。两个组件从 SDK 就绪开始共用一个 30 秒绝对预算。收到已经分类的 Provider
错误回调后，Bridge 不会再重建新组件，但 Turnstile 仍可在该公共预算内完成同一组件上的有界恢复；
一旦要求用户进行可见交互，则立即失败退出。

默认的 `auto` 模式只会在尚未记录 Bridge 安装状态时，才使用本机匹配的 Chromium 系浏览器
兜底。一旦安装并配对了 Bridge，`auto` 会在 Bridge 不可用时直接报错，不会悄悄启动独立浏览器。
只有明确接受独立浏览器兜底时，才使用 `challenge_browser=isolated`。

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
4. 保持扩展启用。无需打开或保留 Suno 标签页。

可使用下面的命令检查 Bridge 通信，不会创建歌曲、运行 Challenge 或消耗额度：

```bash
sunox doctor --browser-bridge
```

扩展安装后，重启 Chrome 仍会保留。扩展清单使用独立的 Bridge runtime build，因此只更新 CLI、
没有修改 Bridge 的版本不会改变扩展包，也不需要 Chrome 重新加载。Sunox 升级并确实包含新版
Bridge 时，刷新本地扩展文件：

```bash
sunox install-browser-extension --force
```

命令会持续记录激活状态，直到 Chrome 认证了完全一致的运行时与配对。首次解压返回
`status=installed`、`reload_required=null`、`runtime_ack_pending=true`、
`pending_origin=load_unpacked` 和 `activation_required=load_unpacked`；完成“加载已解压的扩展程序”
后再运行 `sunox doctor --browser-bridge`。仅有磁盘文件并不会被误报为 Bridge 已就绪。

已认证安装发生更新时，返回 `reload_required=true`、`runtime_ack_pending=true` 和
`activation_required=reload`：只点击一次“重新加载”，然后运行 doctor。从未完成认证或刚恢复的
安装，其 Chrome 状态可能不确定，此时 `activation_required=ensure_loaded`；
`activation_options` 会给出带条件的替代分支，例如 `load_unpacked_if_missing` 或
`enable_and_reload_if_present`。这些分支互斥，不是需要依次执行的步骤。对已经是最新的扩展包，命令
会主动探测 Chrome；精确认证成功后会清除 marker，并返回 `reload_required=false`、
`runtime_ack_pending=false`，且不再要求激活。如果仍无法确认，则返回 `reload_required=null` 和
`runtime_ack_pending=true`；应遵循唯一的 `activation_required` 决策，不要反复点击“重新加载”。

doctor 会区分配对 secret 缺失和可修复的内容损坏，并明确要求执行一次受管的
`install-browser-extension --force` 修复。符号链接、非 UTF-8 数据、不可读路径等不安全或无法访问的
条目会 fail closed，Sunox 不会承诺 `--force` 或“重新加载”可以修复。只更新 CLI、重启电脑或重启
Chrome 本身都不需要重新安装或重新加载 Bridge，也无需刷新 Suno 页面。Sunox 会在 macOS 和
Windows 上自动选择当前用户的应用配置目录；Chrome 使用这个未打包扩展期间，不要移动或删除该目录。

相关覆盖参数如下：

```text
--captcha          即使预检不要求，也强制执行浏览器验证
--no-captcha       禁止自动调用浏览器验证
--token <token>    使用外部已经解出的 Challenge Token
```

`challenge_browser` 支持 `auto`（默认）、`existing`（必须使用 Bridge，绝不启动独立浏览器）
和 `isolated`（始终使用临时浏览器）。单次命令可使用 `-c challenge_browser=existing`。
`existing` 这个名称是为了兼容原有配置，现在表示“使用现有 Chrome Profile 中已安装的 Bridge”。
Bridge 会自动创建并删除绑定 nonce 的 offscreen iframe，不会创建用户标签页或浏览器窗口；
加载固定的 Suno 根地址前，它会先安装受控请求和响应规则。`Location` 已被移除的标准 3xx 响应
可以作为受控文档继续执行；一旦真的离开该承载地址，就会直接失败。
已经安装或配对的扩展缺失、版本过旧、协议漂移或无法连接时，命令会直接报错。`auto` 只有在
尚未记录 Bridge 安装信息时才可能启动独立浏览器；已经安装的 Bridge 一旦被禁用、版本过旧、
无法访问或丢失配对密钥，`auto` 同样会直接停止。只有明确允许独立浏览器时才使用 `isolated`。

无人值守且不能新增 Suno 标签页、也不能启动独立浏览器进程时，安装 Browser Bridge，并去掉
`--no-captcha`。此时 `auto` 和 `challenge_browser=existing` 都会在 Bridge 不可用时直接停止；
即使尚未配置配对信息，`existing` 也会强制要求 Bridge。未安装 Bridge，或者无法确认是否安装时，
应保留 `--no-captcha`，遇到 Challenge 就会在提交前停止。未配置 Bridge 时，仅在默认 `auto`
模式下去掉 `--no-captcha`，仍然允许 Sunox 启动独立浏览器兜底。

安装 Browser Bridge 本身就表示持续允许 Sunox 在其自动管理的短生命周期上下文中执行 Challenge，
每次生成不需要再次确认。“不要留下 Suno 标签页”“不要启动新浏览器进程”“不要显示验证码”等
要求允许使用已安装的 Bridge，并不等于 `--no-captcha`；`challenge_browser=existing` 仍是明确
限定只使用 Bridge 的配置。只有用户明确禁止包括 Bridge 在内的一切 Challenge，或者明确要求传
`--no-captcha` 时，已经安装 Bridge 的机器才保留该参数。

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

配置按默认值、配置文件、环境变量、命令行覆盖的顺序合并，再校验最终值。`config set` 可以
直接修复非法配置项，并保留其他配置键；TOML 语法本身损坏时，仍需编辑错误中指出的文件。

同一账号的写操作默认串行执行，避免刷新认证或修改远端资源时互相覆盖。`--parallel` 会为
当前命令关闭这层保护，只应在确定需要并发写入时使用。

账号写请求发送前，Sunox 会在其配置目录的 `operations/<operation_id>.json` 保存恢复信息。
写命令失败或按 Ctrl+C 时，JSON 错误中的 `details.operation_recovery` 会给出检查点路径、
已知资源 ID 和只读检查命令。Ctrl+C 会停止 CLI，Suno 仍可能完成已提交的任务；再次提交前
请先检查这些资源。检查点只保存允许的标识符，不保存凭证或提示词，也不支持自动续跑。
命令成功后会删除对应的操作检查点。

全局 `--read-only` 会在第一次写请求前拒绝账号写操作。它仍允许账号读取和 prepared download
（后者可能计入下载额度），并禁止时间轴歌词补生成及缺失 WAV/OPUS 的服务端转换；若结果已存在，
仍可只读返回。

部分命令会消耗 Credits 或修改远端资源。新建的歌曲、歌单和 Persona 默认保持私有，只有
显式执行公开命令才会改变可见性；不可恢复的操作必须传入 `-y` 或 `--yes`。

[Suno 已公告](https://about.suno.com/blog/suno-updates-tos)自 2026 年 9 月 3 日起，Pro 每月最多
下载 20 首、Premier 每月 60 首；Premier 的 Studio 导出不受此限制。Sunox 使用官方
prepared-download 流程，不绕过套餐计数。CLI 只展示
实时 billing 响应实际返回的额度字段，不臆测剩余下载次数。

## 开发

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

请从 `main` 新建功能分支，并通过 Pull Request 提交变更。

## 许可证

[MIT](LICENSE)
