# Suno 近期非 Studio 功能与 sunox 缺口（2026-07-15）

## 结论

按 2026-01-15 至 2026-07-15 的 Suno 官方 Release Notes、Help Center 和官网产品页核对，`sunox` 已覆盖 v5.5 基础生成、普通歌词生成、旧式 12 类 generation-backed stems、Personas、音频上传以及常见 clip 编辑，但尚未覆盖下列近期、适合 CLI 化的能力：

1. **P1：新版 Get Stems 三模式**——Auto Split、Split from Mix、Advanced Split。
2. **P1：Sounds 创建模式**——one-shot / loop，以及 BPM、Key。
3. **P1：Sample 与 Mashup**——从片段继续创作、双曲融合。
4. **P1：v5.5 Voices 的“使用已有 Voice”与独立资源语义**。
5. **P1：Custom Models 的列举、显式选择和训练生命周期**。
6. **P2：新版歌词协作能力**——自然语言编辑、Lyricist、词/行 variations 与 references。
7. **P2：Lyrics Model Selector**——当前生成请求固定使用 `default`。
8. **P2：My Taste 可见性与开关**——当前增强标签请求可能已经获得服务端个性化结果，但 CLI 无法确认或控制。

优先建议先做 **Get Stems 三模式**，其次做 **Sounds**，再做 **Sample/Mashup**。这三项均是明确的音频创建/导出工作流，不依赖 Studio；现有 CLI 结构也已有生成、轮询、下载和 partial-mutation 基础设施。

### 实施状态（2026-07-15）

- 已补齐 Remaster 的 `--variation subtle|normal|high`，默认保持 `normal`，并覆盖 CLI 与请求契约测试。
- 其余能力的官方公开资料均未提供私有 Web API endpoint/payload；必须先获得当前 Web HAR 后实现，不能从旧请求或 UI 名称猜协议。

## 范围与证据质量

- 时间窗口：最近约 6 个月，2026-01-15 至 2026-07-15。
- 仅使用 Suno 自己发布的一手来源：官方 [Release Notes](https://suno.com/release-notes)、官方 Help Center、官方产品页。
- 明确排除 Studio 1.2 及其他 Studio 专属能力，符合仓库 [README.md](../../README.md#L229) 的既定范围。
- 官方公开文档描述产品行为、资格和计费，但**没有公开私有 Web API 的 endpoint 或 payload**。因此本文不会把猜测写成接口事实；所有实现项在编码前仍需用当前 Suno Web 会话抓取 HAR，并从当前一方前端 bundle 交叉验证。
- 仓库对照基线来自 [README.md](../../README.md#L136)、[create CLI 参数](../../src/cli/create.rs#L3) 和 [agent-info 能力声明](../../src/commands/agent/info.rs#L251)。

## 缺口明细

### P1：新版 Get Stems 三模式

**官方能力。** Suno 在 2026-06-11 发布 Stem Separation 更新：Auto Split 最多拆成 12 类；Split from Mix 可抽取一个指定乐器或人声，并同时生成其余混音；Advanced Split 可从近 100 种乐器中指定目标。官方 Help 进一步说明 Auto Split 每次 50 credits，Split from Mix 每个目标产生目标/补集两条、总计 20 credits，Advanced Split 为 Premier 专属并按 stem 计费。来源：[Release Notes（2026-06-11）](https://suno.com/release-notes/advanced-stems)、[Advanced Stem Separation（2026-06-12）](https://help.suno.com/en/articles/12702337)。

**sunox 现状。** `sunox clip stems` 没有模式或 instrument 参数；它固定走 `task=gen_stem`、`stem_type_id=91`、`stem_task=twelve`。仓库也明确写明这只是 generation-backed stems，**并非** Web 的 Pro Get Stems export：[create.rs](../../src/cli/create.rs#L334)、[agent-info](../../src/commands/agent/info.rs#L170)、[README](../../README.md#L478)。

**建议 CLI。** 保留旧命令兼容，但把语义显式化：

```text
sunox clip stems <id> --mode auto
sunox clip stems <id> --mode split --instrument vocals
sunox clip stems <id> --mode advanced --instrument guitar --instrument saxophone
sunox clip stems-catalog --json
```

JSON 必须返回 mode、credit estimate、job/action id、生成的 stem 名称与下载 URL；付费前应打印/返回预计 credits，并要求显式参数触发 Advanced 多 stem 请求。

**接口提示。** 官方公开资料未暴露 endpoint。必须重新抓取 Edit → Get Stems 三种模式及完成后下载的网络请求；不能把当前 `/api/generate/v2-web/` 的旧 twelve-stem body 直接扩展后上线。

### P1：Sounds 创建模式

**官方能力。** Suno 于 2026-01-27 发布 Sounds；官方 Help 在 2026-02-18 更新，确认它是 Create 下的实验性模式，可从文本生成两个样本，类型为 One Shot 或 Loop，可选 BPM 和 Key，面向 Pro/Premier。来源：[Release Notes（2026-01-27）](https://suno.com/release-notes)、[Suno Sounds（2026-02-18）](https://help.suno.com/en/articles/10625537)。

**sunox 现状。** 顶层 `create` 只支持 description/custom lyrics/instrumental；clip 子命令中没有 sound/sample 资源类型或 one-shot、loop、BPM、Key 参数：[create.rs](../../src/cli/create.rs#L3)、[README 命令表](../../README.md#L136)。

**建议 CLI。** 新增独立命令，避免把完整歌曲与短样本混在同一参数面：

```text
sunox sound create <prompt> --type one-shot
sunox sound create <prompt> --type loop --bpm 120 --key "A minor"
```

复用现有 challenge preflight、异步 wait 和下载安全限制；JSON 中应明确结果是 sample 而不是 song。

**接口提示。** 官方文档只确认输入和“双结果”，未公开 endpoint、BPM/key 枚举或结果 schema。需抓取一次 one-shot 和一次 loop；再验证是否共用 `/api/generate/v2-web/`、是否有独立 challenge 类型及 credits 字段。

### P1：Sample 与 Mashup

**官方能力。** Suno 于 2026-01-20 发布 Sample 和 Mashup。Mashup 融合两首歌；Sample 从 Suno 歌曲、voice memo 或外部音频中选定片段作为新创作起点。来源：[Sample + Mashup Release Note（2026-01-20）](https://suno.com/release-notes/meet-our-new-create-features-sample-mashup)。

**sunox 现状。** `clip concat` 只把一条已有生成历史拼成 full song；`clip inspire` 只接受一个来源并走 playlist-conditioned loose inspiration；两者都不等价于 Sample 或双来源 Mashup：[agent-info](../../src/commands/agent/info.rs#L189)、[agent-info concat](../../src/commands/agent/info.rs#L196)。当前命令表没有 sample/mashup：[README](../../README.md#L136)。

**建议 CLI。**

```text
sunox clip sample <clip_id> --start 12.5 --end 24.0 [create options]
sunox clip mashup <clip_id_a> <clip_id_b> [--title ...]
```

外部音频应先复用 `clip upload`，再把 upload/clip id 交给 sample，避免新建第二套上传协议。两个命令都会消费 credits，默认只提交一次，不自动批量重试。

**接口提示。** 官方只公开交互入口，没有 endpoint/payload。需要分别抓取 library clip sample、uploaded clip sample、双 Suno clip mashup；重点确认 source range 的单位、是否需要 source clip 权限、结果数量、challenge 和 attribution 字段。

### P1：v5.5 Voices 与现有 Personas 的语义分叉

**官方能力。** Voices 随 v5.5 于 2026-03-26 发布，并在 2026-05-01 的官方指南中明确：可从库内歌曲、实时录音或上传文件创建；上传/录音长度 15 秒至 4 分钟、选取最佳 2 分钟；必须朗读随机短语做本人验证；创建时需要 v5.5，并建议提高 Audio Influence。Voices 仅限 18 岁以上，部分地区不可用。官方 FAQ 同时说明：Voices 没有删除 Personas，Style Personas 仍在 Voices 菜单中。来源：[v5.5 Release Note](https://suno.com/release-notes/introducing-v5-5-voices-custom-models-and-my-taste)、[Voices 使用指南（2026-05-01）](https://help.suno.com/en/articles/11362369)、[Voices FAQ（2026-03-26）](https://help.suno.com/en/articles/11362433)。

**sunox 现状。** `--persona <id>` 和 `persona` 资源命令仍按旧 Persona API/字段工作；CLI 文案把它泛称为 custom voice，但 agent-info 把 voice verification 标为 stale/unconfirmed：[create.rs](../../src/cli/create.rs#L65)、[README persona](../../README.md#L162)、[agent-info](../../src/commands/agent/info.rs#L286)。因此不能确认它能正确列出、区分和使用 2026 Voices，也没有 Audio Influence 参数。

**建议分阶段。**

1. 先做只读 `voice list/info`，并允许 `create --voice <verified_voice_id> --audio-influence <0-100>`；只使用用户已经在 Web 完成验证的 Voice。
2. 之后再评估 `voice create`。随机短语、麦克风、本人比对、年龄/地区/权利确认是安全边界，不能复用旧 `persona create` 假装完成验证，也不能自动勾选权利声明。

**接口提示。** 官方文档未给 endpoint。需要在有资格的账号上抓取 list/select/use existing Voice；创建验证流程需单独进行安全评审。优先验证生成 body 是否仍复用 `persona_id`，以及 Audio Influence 对应哪个字段，不能根据旧命名推断。

### P1：Custom Models

**官方能力。** v5.5 Custom Models 允许 Pro/Premier 用户用至少 6 首自己拥有权利的曲目训练私人模型，最多保留 3 个，训练约 2–5 分钟，完成后出现在模型下拉列表。来源：[Custom Models in v5.5（2026-03-26）](https://help.suno.com/en/articles/11362497)、[v5.5 官方博客（2026-03-26）](https://suno.com/blog/v5-5)。

**sunox 现状。** `sunox models` 能读取 account billing models，但 `create --model` 是封闭的 `clap::ValueEnum`，只接受预置版本；无法显式传入账号自定义模型 id/key，也没有 custom-model 资源命令：[models.rs](../../src/cli/models.rs#L3)、[README 模型表](../../README.md#L447)。

**建议分阶段。**

1. 先让 `models --json` 区分 standard/custom，并允许 `create --model-id <account_model_id>`；继续用 account capability response 做 can_use 校验。
2. 再加 `custom-model create --clip-id ...`、`list/info/wait/delete`。创建必须显式确认素材权利，不自动上传整个目录；至少 6 首及最多 3 个模型应以前端/服务端实时能力数据为准。

**接口提示。** 官方确认 bulk upload 和 2–5 分钟训练，但无 API。抓包应覆盖：模型下拉的列表来源、6 首上传后的 create、训练状态轮询、删除，以及生成时实际传的是 `mv`、model id 还是另一字段。

### P2：新版歌词协作能力

**官方能力。** 2026-07-09 的 Web 歌词更新增加 Lyricist（保存样例以复用风格）、自然语言编辑、词/整行的 Variations 与 References、结构标签和 autosave。来源：[Lyrics improvements on Web（2026-07-09）](https://suno.com/release-notes/lyrics-improvements-on-web)。

**sunox 现状。** `sunox lyrics --prompt` 只提交一个通用 prompt 并轮询完整歌词；没有编辑现有歌词、选区、Lyricist 或 rhyme/reference 资源：[create.rs](../../src/cli/create.rs#L193)、[lyrics API](../../src/api/lyrics.rs#L9)。结构标签不是实质缺口：CLI 已允许用户在 `--lyrics`/`--lyrics-file` 中直接写 `[Verse]`、`[Outro]`。全屏编辑器和 autosave 属于 UI，不需要 CLI 模仿。

**建议 CLI。** 先做无状态、高价值部分：

```text
sunox lyrics edit --file lyrics.txt --instruction "make the second verse less literal"
sunox lyrics variations --text "..." --kind rhyme
```

Lyricist 是有生命周期的账号资源，应在确认 list/create/delete 契约后再做。因为该功能上线仅 6 天，优先等待/采集稳定的 Web 合同，不应扩写旧 `/api/generate/lyrics/` 请求体碰碰运气。

### P2：Lyrics Model Selector

**官方能力。** Suno 2026-05-14 的官方 Mobile App 更新明确加入 Lyrics Model Selector，让用户在多个 lyrics models 中选择。来源：[Release Notes（2026-05-14）](https://suno.com/release-notes)。

**sunox 现状。** 生成请求已有 `metadata.lyrics_model` 字段，但 custom lyrics 路径固定写入 `default`，CLI 没有选择参数；独立 `sunox lyrics` 也只发送 `{prompt}`：[submit.rs](../../src/commands/create/submit.rs#L159)、[lyrics.rs](../../src/api/lyrics.rs#L21)。

**建议。** 先从当前账号能力/前端配置读取可选模型，再提供 `sunox lyrics --model <key>` 和 `create --lyrics-model <key>`。官方只明确 mobile surface，需验证 Web/后端账号是否同样开放，避免把移动端 UI 枚举硬编码到 CLI。

### P2：My Taste 的可见性与控制

**官方能力。** My Taste 向所有用户开放，根据收听和创建习惯，在 Magic Wand/Style Augmentation 时生成个性化 style description；默认开启，用户可查看、编辑或关闭。来源：[My Taste（2026-03-26）](https://help.suno.com/en/articles/11362561)、[v5.5 Release Note](https://suno.com/release-notes/introducing-v5-5-voices-custom-models-and-my-taste)。

**sunox 现状。** `--enhance-tags` 已调用 prompt upsample，并在生成 metadata 中写 `personalization_enabled=true`，但 `/api/prompts/upsample` 的当前 response type 没有告诉调用方结果是否真的应用了 My Taste；CLI 也不能查看/编辑/关闭账号 My Taste：[generation.rs](../../src/api/types/generation.rs#L174)、[README](../../README.md#L573)。

**建议。** 先做一次启用/禁用 My Taste 的双向 HAR 对照，确认差异发生在 upsample request、账号设置还是服务端隐式状态。若 API 支持 per-request override，再为 `--enhance-tags` 增加 `--personalization on|off|account-default`，并在 JSON 输出中回显实际状态；不要仅凭现有 submit metadata 宣称“已经支持 My Taste”。

## 不建议纳入本轮 CLI 路线图

- **Studio 1.2**：明确超出项目范围。
- **自定义足球 anthem、iOS Notes/Voice Memos 分享、CarPlay/Android Auto**：官方标注为 mobile/系统集成，不是通用 CLI 能力。
- **Create Memory**：本质是表单记忆；CLI 已可通过 shell history、配置和脚本实现，价值低。
- **Profile 管理、Pin Favs、社交功能**：当前项目边界集中在音乐创建和资源管理；除非用户单独扩展社交 scope，否则不应挤占上述音频能力优先级。
- **全屏歌词编辑器和 autosave**：是 UI 体验，不应在 CLI 中复刻。

## 推荐实施顺序与验证门槛

| 顺序 | 能力 | 原因 | 上线前最低证据 |
|---:|---|---|---|
| 1 | Get Stems 三模式 | 现有命令名称容易让用户误以为已覆盖新版 Web 能力；音频导出价值最高 | 三模式 HAR、credits/plan 错误、poll/download、无目标乐器失败样例 |
| 2 | Sounds | 参数天然适合 CLI，且与完整歌曲边界清晰 | one-shot + loop HAR、BPM/key 枚举、两结果 schema、challenge |
| 3 | Sample + Mashup | 创作价值高，复用现有 clip/upload/wait 基建 | 三类来源抓包、range 单位、双源权限、attribution |
| 4 | 使用已有 Voice | v5.5 的核心个性化能力；先避开身份验证自动化 | list/select/use HAR、v5.5 限制、Audio Influence 字段、地区/年龄错误 |
| 5 | 显式选择 Custom Model | 当前封闭 model enum 是结构性阻塞 | account model schema、生成 body 的真实模型标识 |
| 6 | 新歌词编辑 | 最新功能，但刚上线，合同漂移风险更高 | generate/edit/variation/Lyricist 分别抓包及错误样例 |
| 7 | Custom Model 训练 | 多步、有权利声明、会创建长期资源 | upload/create/poll/delete 全链路与 partial mutation 恢复设计 |
| 8 | My Taste / lyrics model | 需先确认 Web 和账号实际合同 | 开关对照 HAR、可用模型枚举来源 |

所有新增写操作继续遵循项目现有的 account-scoped serial mutation、语义退出码、challenge preflight、structured partial failure 和“付费/公开/破坏性操作必须显式请求”策略。
