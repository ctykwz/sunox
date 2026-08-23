# Sunox 待确认协议复核（2026-08-23～24）

## 结论

本轮用 Suno 官方当前 Web bundle 及其交互、路由懒加载分片复核了此前待确认的五组协议。结果如下：

| 协议组 | 当前结论 | 是否仍需账号写验证 |
|---|---|---|
| Cowrite submit | **当前一方源码已确认，且最小真实账号提交成功；观察到 credits delta = 0** | 否 |
| Persona generation | **Advanced 的 detail/source/null/model 映射已由一方源码确认；修复后的私有 rootless Vox Advanced 真实生成亦已完整成功** | 否；仅证明旧 bare custom 短请求兼容性时才需另测 |
| Persona mutation | **create/edit/visibility/love/trash/restore/purge 均有当前一方源码** | 否 |
| Inspiration tag upsample | **当前 Web 为显式 Enhance 操作，不是 Inspiration 的必经步骤；Sunox 当前可选 `--enhance-tags` 与之相符** | 否 |
| MP3/M4A/WAV/OPUS download | **当前一方源码完整确认，包括 WAV/OPUS 都是 GET-first、缺失才转码** | 否；只有验证某个账号下的实际转码结果才需要 mutation |

因此，五组协议均不应再标为“缺当前协议证据”。Persona Advanced 不仅已有一方源码映射，还完成了 rootless Vox 的账号端到端验证；唯一仍有条件性不确定的是：如果继续保留复核前的 `create/describe --persona` 短 payload，它是否仍被后端作为兼容输入接受。这个问题只涉及旧短路径，不影响当前 Advanced 适配结论。

本轮经用户授权执行了两类受控写：一次最小 Cowrite submit（成功，观察到 credits delta = 0）和一次私有 rootless Vox Advanced generation。后者按修复后的 contract 提交，使用 v5.5（`chirp-fenix`）与 `task: "vox"`，返回两个 `complete`、带 `audio_url`、`is_public:false` 且 `persona_id` 匹配的 clip；即时 credits 从 2107 降到 2097，精确 delta = 10。随后 paginated Persona clips 只读回查也确认两个结果已持久化。报告不记录账号、Persona 或 clip 标识；没有上传、Persona/clip/playlist 元数据修改、可见性/发布、关注/收藏、删除或 WAV/OPUS 转码。

## 证据方法

- 第一方入口：[Suno Create](https://suno.com/create)。
- 抓取并检索当前页面的初始分片、页面声明的全部递归交互分片，以及 `/voice` 路由专属分片。
- 初始/交互依赖图最终无未下载的已声明 JS 分片；`/voice` 路由专属分片单独核验。
- 2026-08-24（Asia/Shanghai）先用当前已配置账号只读调用 `GET /api/persona/get-personas/` 与 `GET /api/persona/get-persona/{persona_id}/`，交叉确认 legacy/rootless Vox 的真实 response shape；随后仅执行上述授权 Cowrite submit 与私有 Advanced generation，并用 paginated GET 做持久化回查。
- 以下 byte offset 均为原始 minified 文件的字节偏移；SHA-256 用于固定本次证据快照。

| 官方分片 | SHA-256 | 主要证据 |
|---|---|---|
| [`2vyct5q4gq553.js`](https://suno.com/_next/static/immutable/chunks/2vyct5q4gq553.js) | `7f54ece7f0888ff20c038a97931b78ba91e8aa8f3ae16e229159f678b2ac26ab` | Persona generation、tag upsample、Persona create/love |
| [`2o55p_0ruo1em.js`](https://suno.com/_next/static/immutable/chunks/2o55p_0ruo1em.js) | `abe3ea475ec66dfd8735d49d1a08c4c3fa9fcbab1299655323d4d9e9d5fd909e` | `EMPTY_UUID` 当前精确值 |
| [`3glkvxw_k-y2j.js`](https://suno.com/_next/static/immutable/chunks/3glkvxw_k-y2j.js) | `581825334acf60efb854dde6f94e01375c93afabb7a49843f60515d82e50ceba` | Cowrite submit |
| [`0psavgrakzykm.js`](https://suno.com/_next/static/immutable/chunks/0psavgrakzykm.js) | `e2e001380b49d4620078290007dac6468e27f94b496fc5820455f9ea5e0b6b41` | MP3/M4A/WAV/OPUS download |
| [`24l-fzxvb5otv.js`](https://suno.com/_next/static/immutable/chunks/24l-fzxvb5otv.js) | `3d3a5a3a647b8cd60eb2f0ac0d4e536deec18f0fc9c3f2c5c892c19e3a4e21cf` | Persona bulk trash |
| [`3spz_3hruuf8x.js`](https://suno.com/_next/static/immutable/chunks/3spz_3hruuf8x.js) | `fc8c28e1ea045b3380710a714bf6e1b8835479b273fd41c3683c49e89904e4dd` | Persona per-item trash/restore/delete |
| [`2x-65bipljzoj.js`](https://suno.com/_next/static/immutable/chunks/2x-65bipljzoj.js) | `476de78ecc690115872d049af7e1030b1f6e52f38e380982dfc5dad0dee3fcd9` | Persona edit mutation hook |
| [`3iglu18q2jo7v.js`](https://suno.com/_next/static/immutable/chunks/3iglu18q2jo7v.js) | `012286bf7496160dc099ac988d1ffe8d5fce44881714b03e18481d870c78e063` | 完整 Persona edit body |
| [`24xq6d54vzip9.js`](https://suno.com/_next/static/immutable/chunks/24xq6d54vzip9.js) | `e5c54cdc72714aa9e5eeed5fade6c47929671df1b23c4939b76c3f5c7472c6fb` | `/voice` 页面 visibility 与轻量 edit |

## 1. Cowrite submit：当前协议已确认

官方交互分片 `3glkvxw_k-y2j.js` byte 8,735 直接调用：

```http
POST /api/generate/cowrite-lyrics/
```

当前 body 为：

```json
{
  "selected": "...",
  "context_before": "...",
  "context_after": "...",
  "instruction": "...",
  "title": "...",
  "style": "...",
  "mode": "apply_user_request | generate_variants",
  "references": [],
  "num_variants": null,
  "lyricist_id": null,
  "metadata": {
    "lyrics_model": "default | <model id>",
    "enable_thinking": false
  },
  "create_session_token": null,
  "lyrics_project_id": null
}
```

同一函数读取 `edited_lyrics`、`lyrics_request_id`、`lyrics_id`、`variants`、`artist_to_tag_mapping` 和 `next_prompts`。这与 `src/api/lyrics.rs` 的 fresh-generation 请求及 `CowriteLyricsResponse` 字段一致。此前只下载初始 create graph 时仅看到 models GET，是因为 submit handler 位于交互懒加载分片，并非协议已移除或变成异步任务。

2026-08-24 又用当前账号执行了一次最小 Cowrite submit：请求成功，响应按当前 `CowriteLyricsResponse` 解码，提交前后 credits 读回的观察差值为 0。这一 live 结果证明当前账号上的后端可用性；endpoint、body 和 response contract 本身此前已由当前一方 bundle 确认。

## 2. Persona generation：两条当前 Web 协议并存

### Advanced / Custom reference 路径

`2vyct5q4gq553.js` 当前构建逻辑会先读取 Persona 类型与来源，再生成 Persona reference：

- byte 504,251 附近：根据 `selectedVersion` 与模型能力选择 `vox` 或 `artist_consistency`；
- byte 1,867,000 附近：`getGenerateTaskFromReferences` 将 Persona variant 解析为 `task: "vox"` 或 `task: "artist_consistency"`；
- byte 1,872,038–1,872,065：将 source 写入 `artist_clip_id`，并写入 `persona_id`；
- 同一 builder 还写入 `artist_start_s`、`artist_end_s`，并设置 Persona 对应的 `override_fields`。

也就是说，Advanced Web 路径不是只有一个裸 `persona_id`，而是一个带类型、source 和 task 解析的 reference contract。

这里的 task 不能只按 `persona_type` 二选一。当前 `modelValidForVoxPersona` 仅接受 external key 含 `crow/custom/dodo/eagle/fenix/goose/hawk/ibis` 的模型（byte 1,231,831）：

- Vox Persona + 合法 source + Vox-capable model：保留 `selectedVersion: "vox"`，最终 `task: "vox"`；
- Vox Persona + 合法 source + 较旧模型：Web 去掉 Vox variant，回退 `task: "artist_consistency"`；
- rootless Vox：reference 必须保留 Vox variant，但旧模型会被 `VOICE_REQUIRES_V5` 阻断，不能提交；
- legacy/未声明类型 + 合法 source：`task: "artist_consistency"`。

因此，CLI 若允许显式选择旧模型，不能对所有 `persona_type: "vox"` 无条件发送 `task: "vox"`。安全做法是复刻上述 fallback；对 rootless Vox + 非 Vox-capable model 应 fail closed，或者在用户未显式锁定模型时明确解析到可用的 V5+ 模型。

### Advanced 的 `artist_clip_id` 来源与空值规则

官方 builder 不是直接从 Persona API response 任取一个 clip 字段写请求，而是先投影到 create state，再构造 reference：

```text
Persona state: clipId || rootClipId
    -> Persona reference.clipId
    -> artist_clip_id（仅 reference.clipId 为 truthy 时赋值）
```

`2vyct5q4gq553.js` 的具体证据为：

- byte 504,282/504,784 与 1,234,312：Advanced reference 读取 `clipId || rootClipId`；
- byte 1,168,010/1,168,044：正常 picker 从完整 Persona detail 加入 create state 时，`rootClipId` 与 `clipId` 都只取 `detail.root_clip_id`；
- byte 1,168,738：另一个独立的 `applyPersonaFromData` helper 使用 `detail.root_clip_id || detail.clip?.id`。这是入口特例，不是 normal picker 的通用 fallback；
- byte 1,168,272：`persona_clips[0].clip.id || vocal_clip_id` 只保存成 `agenticFallbackClipId`；Advanced reference builder 不读取该 fallback，因此 **`vocal_clip_id` 不是 Advanced `artist_clip_id` 的来源**；
- 从一首带嵌套 Persona 的 clip 发起时，create state 会把当前 `clip.id` 放进 `clipId`，所以该 clip-origin 入口最终可使用当前 clip ID。这与 Persona-detail 入口优先 root clip 并不矛盾。

当前账号的只读 detail 把这个边界实际区分开了：legacy Persona 的 `root_clip_id` 与嵌套 `clip.id` 相同；rootless Vox 则同时返回 zero UUID `root_clip_id`、非空 `clip.id`、非空 `vocal_clip_id` 和非空 `persona_clips`。这些 Vox 字段都不应被 normal picker 错当成 `artist_clip_id`。因此，复刻正常 picker 的安全映射是：只使用合法 `root_clip_id`；rootless Vox 忽略嵌套 `clip.id`/`vocal_clip_id`/`persona_clips`，保留空 source。只有明确复刻 clip-origin 或 `applyPersonaFromData` 特定入口时，才分别使用当前 clip ID 或该 helper 的 `root || clip` 规则。

range 同样不能从 Persona 的 vocal range 推导。normal picker 使用 `artist_start_s = 0`、`artist_end_s = rootClip.metadata.duration`；rootless Vox 为 `0/null`。当前账号的 legacy detail 同时返回了非零 `vocal_start_s/vocal_end_s` 和更长的 root clip duration，官方 builder 取后者的完整 `0..duration`，完全不读 vocal range。`applyPersonaFromData` 特例则是 `0/null`。

空值与 zero UUID 也有明确的一方规则：

- `2o55p_0ruo1em.js` byte 17,219 定义 `EMPTY_UUID = "00000000-0000-0000-0000-000000000000"`；
- `0psavgrakzykm.js` byte 12,364/13,858 的 `isValidClipId` 等价于 `!!(id && id !== EMPTY_UUID)`；
- `2vyct5q4gq553.js` byte 860,142 的 Persona detail hook 会把无效 root 归一为 `null`；当前账号的 list 读回也将 rootless Vox 显示为 `null`，而 raw detail 读回仍可给出 zero UUID。正常添加流程只允许“有合法 root”，或“`persona_type === "vox"` 且 rootless”两种情况；
- generation request 基线把 `artist_clip_id`、`persona_id`、`artist_start_s`、`artist_end_s` 初始化为 `null`；Persona reference 没有 `clipId` 时不会覆盖 `artist_clip_id`；
- byte 1,866,153 的发送前 validator 若发现 `artist_clip_id === EMPTY_UUID` 会直接抛错，不发 generation 请求。

最终边界是：rootless Vox 使用 `task: "vox" + persona_id`，`artist_clip_id` 保持 `null`；legacy/`artist_consistency` 必须有合法 root source，否则应 fail closed；zero UUID 只能先归一为“无 root”，不能作为 clip ID 发送。

补充：Stack 模式会把 Persona source 放在 `metadata.stacked_task_clips[].id`，同时携带 task/range/`persona_id`；普通非 Stack Advanced reference 才投影为顶层 `artist_clip_id`。Sunox 当前普通 create/describe 不实现 Stack，这不改变其非 Stack 适配规则。

### Simple agentic 路径

同一官方分片也明确保留另一条合法路径：

- byte 1,240,493：Simple agentic state 设置 `agenticPersonaId`；
- source clip 存在时还设置 `agenticPersonaVoiceRef` 并可加入 `audio_refs`；
- byte 1,874,730 附近：payload 写入裸 `persona_id`；
- byte 1,874,947：可选写入 `persona_voice_ref`；
- 这条路径不通过 Persona reference resolver 强制生成 `vox`/`artist_consistency` task。

所以“裸 Persona 一定错误”与“所有 Persona 都只需裸 ID”都不准确。当前 Web 的真实边界是：

```text
Simple agentic -> persona_id [+ persona_voice_ref/audio_refs], no Persona task resolution
Advanced      -> Persona reference -> vox|artist_consistency + source/range fields
```

### Sunox 基线与适配边界

本轮复核前的普通 `create/describe --persona` 基线只写入 `persona_id` 与 `override_fields=["prompt","tags"]`，不解析 Persona detail、selected version、source clip/range 或 `persona_voice_ref`。该基线因此不等同于 Advanced Web builder；同时也不是完整复刻 Simple agentic state builder。当前实现已经切换为上文 detail/source/null/range/model fallback 规则，并有 request-shape 测试覆盖。

### Advanced 账号端到端闭环

2026-08-24 使用一个私有 rootless Vox Persona（detail `root_clip_id` 为 zero UUID）按修复后的 Advanced contract 做了一次授权生成：

- 后端解释为 `task: "vox"`，模型为 v5.5（`chirp-fenix`）；
- 返回两个 clip，均为 `complete`、有 `audio_url`、`is_public:false`，且 `persona_id` 与所选 Persona 匹配；
- 提交前即时 credits 为 2107，完成后为 2097，精确 delta = 10；
- 随后 `GET /api/persona/get-persona-paginated/{id}/?page=1` 成功，返回 `total_results=12`、12 个 `persona_clips`；两个新 clip 均再次读回为 complete/private、`task:"vox"` 且 Persona 关联一致。

这条证据把构建规则、后端 task/model 解释、完成态媒体、隐私、额度变化与持久化读回连成了完整闭环，同时没有发布或修改 Persona。报告不保留任何 Persona/clip/账号标识。

当前 Advanced rootless-Vox 路径已经不再需要账号写验证。只有在要继续宣称旧“bare custom”短 payload 是受支持的后端兼容协议时，才需要另做一次有明确授权的生成；那是旧兼容路径问题，不影响已闭环的 Advanced 结论。

## 3. Persona mutation：当前路由均已找到

当前官方源码确认：

| 操作 | 当前 route | 一方证据 |
|---|---|---|
| Create | `POST /api/persona/create/` | `2vyct5q4gq553.js` byte 861,679 |
| Edit | `PUT /api/persona/edit-persona/{persona_id}/` | `2x-65bipljzoj.js` byte 25,733 |
| Visibility | `PUT /api/persona/set_visibility/{persona_id}/?is_public=<bool>` | `/voice` 分片 `24xq6d54vzip9.js` byte 2,906 |
| Toggle love | `POST /api/persona/{persona_id}/toggle_love/` | `2vyct5q4gq553.js` byte 1,888,350 |
| Bulk trash/restore/delete | `PUT /api/persona/bulk-trash-personas/` | `24l-fzxvb5otv.js` byte 2,563 |
| Per-item trash/restore/delete | `PUT /api/persona/trash-persona/{persona_id}/?undo=<bool>&hide=<bool>` | `3spz_3hruuf8x.js` byte 3,093 |

Bulk 与 per-item route 当前同时存在。两者都使用：

```text
trash   -> undo=false, hide=false
restore -> undo=true,  hide=false
delete  -> undo=false, hide=true
```

`edit-persona` 当前至少存在两种 UI body 投影：

- 完整 Voice modal 会携带 `persona_id`、name、description、`image_s3_id`、`is_public`、`persona_type`、`user_input_styles` 及适用的 vox/range 字段；
- `/voice` 详情页轻量编辑携带 `persona_id`、name、description、`image_s3_id`。

`image_s3_id` 应保留：`24xq6d54vzip9.js` 的编辑 state 从当前 Persona 的 `image_s3_id` 初始化，保存时继续发送该值。Sunox `build_edit_persona_request` 的 `args.image_s3_id.or(current.image_s3_id)` 与当前 Web 行为一致。

注意：只递归首页/create graph 会漏掉 `/voice` route-specific visibility hook，不能据此推断 `set_visibility` 已移除。以 `/voice` 分片为准，Sunox 当前 publish/unpublish route 是适配的。

安全边界：上述 route、method、query/body 已有当前官方源码，不需要用真实账号改 Persona 来“发现协议”。真实 mutation 只验证账号权限、业务状态变化和响应，不应作为无必要的协议探针。

## 4. Inspiration 与 tag upsample：增强是显式可选动作

`2vyct5q4gq553.js` byte 705,908 的当前样式增强请求为：

```http
POST /api/prompts/upsample

{
  "original_tags": "...",
  "lyrics": "... | omitted",
  "is_instrumental": "<current create state>",
  "user_guidance": "... | omitted"
}
```

响应继续提供 `request_id` 和 `upsampled`；byte 706,700 使用模型 feature `tag_upsample` 控制该按钮。调用点是样式栏显式 Enhance/Cowrite 操作，当前 Inspiration reference 构建与 generation builder 不会因为存在 Inspiration clip 而自动调用该接口。

Sunox 当前 `clip inspire` 的 `--enhance-tags` 是显式 opt-in：未传时直接使用提供的 tags，不读 personalization settings、不调用 upsample；传入时才先校验 `tag_upsample` feature，再 `GET /api/personalization/settings`。`styles_augmentation` 缺失时按当前 Web 的 `?? true` 语义视为启用；随后 POST upsample（当前 vocal Inspiration 发送 `is_instrumental:false`），写入 `metadata.last_tags_generation` 的 tags/request/original/personalization 状态，并按解析后的模型重新校验长度。这与当前 Web 的可选边界一致，不需要 submit capture 来确认“是否强制”。

## 5. Download、WAV 与 OPUS：不是历史残留，当前 bundle 仍在使用

官方 `0psavgrakzykm.js` 当前确认：

```http
GET  /api/download/clip/{clip_id}?format=mp3
GET  /api/download/clip/{clip_id}?format=m4a

GET  /api/gen/{clip_id}/wav_file/
POST /api/gen/{clip_id}/convert_wav/

GET  /api/gen/{clip_id}/opus_file/
POST /api/gen/{clip_id}/convert_opus
```

关键字节位置：MP3/M4A 18,194/18,778；WAV read 24,065、convert 23,007；OPUS read 23,119、convert 23,274。

当前 WAV 与 OPUS 都是 GET-first，而不是无条件启动转换：

1. 先 `GET wav_file/` 或 `GET opus_file/`；
2. 已有对应 URL 时直接返回；
3. 缺失时才 `POST convert_wav/` 或 `POST convert_opus`；
4. 最多 24 次、每 5 秒轮询对应 GET。

OPUS helper 还被当前 Studio 初始化路径调用，因此该 route 不是只存在于未引用的兼容死代码。Sunox 对 WAV/OPUS 同样先读 existing URL，缺失才启动 conversion；轮询参数由 CLI 配置控制，协议顺序一致。

MP3/M4A 当前都走 prepared-download GET。Web 对 MP3 在特定 gate 关闭时可回退 `clip.audio_url`，M4A `404/not_found` 可回退 MP3；这些是容错策略，不改变 route contract。`media_urls`/presigned/DRM 是并存的播放或下载优化路径，也不表示 prepared-download route 已废弃。

安全边界：bundle 已足够确认 endpoint 和状态机。若只想验证现有 WAV/OPUS，可安全做对应 file GET；一旦 URL 缺失，继续 convert POST 就是 mutation，只有用户明确要求实际文件时才应执行。

## 最终待办边界

无需再做账号写验证：

- Cowrite endpoint/body/response；
- Persona Advanced rootless-Vox contract（已完成真实生成与 paginated 持久化读回）；
- Persona create/edit/visibility/love/trash route family；
- Inspiration 的 upsample 可选性；
- MP3/M4A/WAV/OPUS route 与轮询顺序。

本轮协议确认已经没有必须继续执行的账号写验证。只有一个条件性边界：

- 如果未来仍要保留或恢复复核前 bare custom 路径，才需要另做一次真实生成证明该旧 payload 是否仍被后端兼容接受，以及它实际解析为哪类 Persona task、生成结果与 credit delta；这不影响已 live 闭环的 Advanced 路径。

此前 `get-persona-paginated` 在 JWT refresh 后出现过间歇 transport reset，普通 detail 对照也同时失败；随后 bounded retry/再读已经成功，并解码非空页面及两个新结果。因此该只读 live 边界已关闭，早期 reset 只保留为可用性证据，不再被解释为 route/schema 不兼容。

Remaster 仍是账号资格边界：当前账号所有可见 Remaster model 均为 `can_use:false`，CLI 在 POST 前正确阻断。本轮没有绕过资格、没有调用 Remaster mutation，也不声称 live Remaster 成功；只有具备 `can_use:true` 的账号才适合做后续端到端验证。
