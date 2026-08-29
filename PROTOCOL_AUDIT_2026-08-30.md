# Sunox / Suno Web 深度协议审计 — 2026-08-24～30

## 结论

本轮把 Suno 官方公告、2026-08-30 登录态 `/create` 页面及其完整可达
frontend bundle、当前 Pro 账号只读 billing 回读与 2026-08-23 基线做了交叉复核。

当前只有一项需要在 **2026-09-03 前按 P0 迁移**：下载已从“直接请求文件”变为
“按 clip 解锁授权，再请求一个或多个格式”。新的必要协议为：

```text
clip.is_download_unlocked === true
    -> 可以直接 GET 文件

clip.is_download_unlocked !== true
    -> POST /api/download/authorize
       {"item_id":"<clip id>","item_type":"clip"}
    -> data.ok === true 后，才 GET 文件
```

授权响应当前 Web 明确读取 `ok`、`reason`、`message`、`credit_deducted`；billing
明确读取 `download_usage.current_period_downloads_limit`、
`current_period_downloads_used`、`additional_download_remaining`，并读取
`download_credit_packs`。这不是可选 UI 增强：Suno 官方已宣布自 9 月 3 日起对所有下载
计数，Pro 为每月 20 次，包括此前生成的歌曲。因此，旧的直接下载路径不能继续作为唯一
协议。

其余核心 CLI 协议本轮没有发现需要立即迁移的 route/version：

- generation 仍是 `POST /api/generate/v2-web/`；
- Cowrite 完整 submit 仍是 `POST /api/generate/cowrite-lyrics/`，响应字段未见迁移；
- Cover Art 仍使用 `/api/video_gen/model-configs`、动态 cost、image/video batch
  submit、pending/history/poll；没有出现可据以替换现有 `SONG_COVER_ART` contract 的
  新版 route；
- Remaster 当前 Pro 账号仍返回 v5.5/v5/v4.5+ 三个 selector，未发现新 endpoint；
- 新 bundle 出现的 `/api/gen/{gen_id}/unlock-preview` 是 staff preview 工具，
  `/api/forbidden` 是不可解码音频 URL sentinel，都不是 CLI 现有接口的迁移目标。

当前页面已经投放 “V6 is here” 促销文案和动态 info-chip，但当前 Pro billing 只读回读
尚未返回 V6 generation model，官方 release notes 也尚未发布 V6 的请求 external key、
能力表或生成协议。bundle 中的 `chirp-goose` 早于本周已存在，且没有一方证据证明它就是
V6。**现在不应把 `chirp-goose`、`Aura-1` 或促销文案猜成 V6 请求协议。**

官方同时宣布“新模型上线时，所有旧模型将退役”。这构成近期高风险迁移信号，但尚没有
可安全编码的替代 schema。CLI 应继续以 billing 返回的 `external_key`、capabilities、
features、limits 与默认模型为准，不应提前硬编码一个猜测的 V6 key。

## 优先级

| 优先级 | 处理项 | 结论 |
|---|---|---|
| **P0，9 月 3 日前** | 下载授权与额度协议 | 必须适配 `is_download_unlocked`、`POST /api/download/authorize`、四个已确认响应字段、三个 `download_usage` 字段和 `download_credit_packs` |
| **P0，安全语义** | 授权重试和批量计数 | authorize 是可能扣下载额度的写请求；网络结果不明时不得自动重试；应按唯一 clip 解锁一次，而不是按输出格式授权多次 |
| **P0，下载格式与 Stems** | prepared route 与 source 解锁 | 主下载优先使用 `format=mp3|m4a|wav|mp4`；legacy WAV/direct video/OPUS 只能在 source 已解锁后兼容，Stems 必须复用 parent clip 的一次解锁 |
| **P1** | 模型退役兼容 | 所有生成入口动态解析 billing model；对服务端不再返回的旧 selector 明确报错，不静默换模型 |
| **P1** | 删除静态 auto fallback | billing 暂时不可达时不得把 `auto` 改写成旧 `chirp-auk-turbo` 后提交；应在写请求前 fail closed |
| **P1** | 下载额度可观测性 | `capabilities`/`account` 输出展示 period limit、used、additional 和 packs；授权扣额度后刷新 billing |
| **P2，新增能力** | 非破坏性补齐 | Rhymes、Remaster 扩展控制、existing-clip Vox、MIDI/key、Cover Art 管理等是增量功能，不是现接口失效 |
| **P2 / 等官方 schema** | V6 | 监控 release notes 与 Pro billing；只有拿到 external key、capabilities 和真实响应后再加 selector/别名 |
| **无需迁移** | generation、Cowrite、Cover Art、Remaster | 当前一方 bundle 未见 route version 替换；维持动态模型/类别发现和已有请求结构 |

## 范围、方法与安全边界

仓库基线是 commit
`b799b42aaecf6afc826bdd1d1ef32509d5e230dd`（`Complete Sunox 0.3.0 release notes`）。

证据标签：

- **OFFICIAL**：Suno 官方 release notes 或官方 blog；
- **BUNDLE**：从 `https://suno.com/create` 登录态页面下载的一方 immutable JS，
  只做文本解析，byte offset 是原始 minified 文件偏移；
- **LIVE-READ**：当前已配置账号的只读 `GET /api/billing/info/` 归一化结果；
- **BASELINE**：仓库中 2026-08-23～24 的协议审计及当时固定的 immutable bundle；
- **INFERENCE**：由已确认事实推导的实施建议，不当作未捕获的后端 schema。

本轮没有执行 frontend JavaScript；仅下载并文本检索 JS bundle。登录态 `/create` 的递归
依赖闭包共 353 个 JS 文件，并校验页面/分片声明的 258 个分片 basename 均已下载，未发现
闭包缺口。本轮没有调用 generation、download authorize、download conversion、upload、
metadata、playlist、Persona、reaction、trash/delete 或其他账号写接口，也没有消费生成或
下载额度。

未登录的 `https://suno.com/create` 会跳到主页；本报告的 create 协议证据来自登录态
`/create`（响应 `X-Matched-Path: /create`），不是从主页营销 HTML 推断。

## 当前证据快照

抓取时间为 2026-08-30 02:02～02:03 Asia/Shanghai；登录态 `/create` 的 HTTP `Date`
为 2026-08-29 18:03:29 GMT。

| 证据 | SHA-256 / 大小 | 主要用途 |
|---|---|---|
| 登录态 `/create` HTML | `f56f57e4da9c7458a376151fe9d26af2de14290ab2b3c9e0acacdced9d264c45` / 122,245 bytes | 当前路由、localization 与递归分片入口 |
| 官方 release notes HTML | `ff837076914379486b725acfaafb8e0da594db566666f01963c35decfd105e5e` / 353,247 bytes | 公开发布项时间边界 |
| `1ow-45tja2vjr.js` | `94c69afc868785b4ffb9042a72c3ac95173c15900051b9d392972ec1e55bd598` / 33,696 bytes | 下载解锁、额度、top-up 与格式下载主证据 |
| `3od223yrej4sl.js` | `6dd05dc209ea183f03eafdafbc3cd1c9fa08d4952816af77ab0fe4fdf8b9f897` / 1,982,458 bytes | 当前 create/generation builder、现有协议面 |
| `33uyx1goiop-7.js` | `4f5821a404b934bcc07cc02e1fcfcf74453e92196a04f6388b487ba4bc80aa3b` / 30,229 bytes | 当前 `POST /api/generate/v2-web/` 提交 |
| `0td1i5ua_mp61.js` | `b8165ff8449618736fc8445d9e17e9b49a5b6152bdc97694032528484b447f6c` / 86,123 bytes | 完整 Cowrite submit 与 rhymes |
| `22xm7pc-z1o96.js` | `9d59983d8fbfd561c80e9e3fa2b90a639ed11b880d30fd786f8666af710e1486` / 3,558 bytes | V6 info-chip GET 与倒计时 UI |
| `15yl1sqicp-3l.js` | `548f46d879e5bc907d71486bcf83b2dc0223a40b9f8dd5a2119549dcc54eaafd` | `DEFAULT_GOOSE_MODEL_NAME="chirp-goose"` 常量 |
| Pro billing 只读归一化结果 | `28397471afe8aa3eea3450e000b3287a97f1ab2437f146cbb0c2eee8aebac802` | 当前账号 plan/model/remaster 能力；不记录账号标识 |

immutable bundle URL 形如：
`https://suno.com/_next/static/immutable/chunks/<chunk name>`。例如下载主证据为
[`1ow-45tja2vjr.js`](https://suno.com/_next/static/immutable/chunks/1ow-45tja2vjr.js)。
文件名与 hash 同时记录，避免以后部署替换时把新内容误当成本次证据。

另一条独立 `/create` 递归文本审计捕获到 deploy
`dpl_Cg9qEo9dDyGJkqA8M2a1e6EQtvtH`，HTML SHA-256
`fbab28758bece49a27bfa64198cfb04f3b6d59509ae474756e84619f0e934045`，共解析
276 个可达 chunk / 329 条 route。它在另一组 immutable chunk 中独立确认了同一下载协议：

- `1q6fi04nwykwu.js`，SHA-256
  `9558e295457e40b5b9224d56af75cd09a47c4aa0e5c762df542b76a9572bdfb2`：
  download restriction / authorize；
- `3g_jlvr463wjj.js`，SHA-256
  `1eafc294ce54e57de7cefce33588fdb1683ba518cacbc310adacba69be15a631`：
  `createClipDownloadHandler`；
- `0qy-q_ymx-ju0.js`，SHA-256
  `32b16e42126491068a5b265f0373bb4ce7f87796671b9797040fc74f27949580`：
  Stems parent-clip gate；
- `1f2eydxud71ej.js`，SHA-256
  `3042027de1b8a0490fe78ca91cc04ebc480bcf9c92243c600a0b426c575e1e0d`：
  legacy WAV helper。

两次抓取的 HTML/chunk 名称不同，说明部署或灰度闭包存在差异；但 exact route、body、gate
和 billing 字段一致，因此下载结论不是单一 chunk 偶然命中。

## 1. P0：下载协议已经改变

### 1.1 官方生效边界

Suno 官方 2026-08-10 公告
[`An update to our downloads policy and Terms of Service`](https://suno.com/blog/suno-updates-tos)
明确说明自 **2026-09-03** 起：

- Free：终身最多 7 次 trial downloads；
- Pro：每月 20 次；
- Premier：每月 60 次；
- Premier + Studio：不受下载次数限制；
- 超出额度后可以购买额外 downloads；
- 限制适用于 9 月 3 日之后发生的所有下载，包括此前生成的歌曲；
- 付费套餐下载的歌曲继续保留官方说明的商业使用权。

这份公告给出了产品/计费生效时间，但没有公开内部 API schema。下面的 schema 来自同日
部署的当前一方 bundle，不由公告文字猜测。

### 1.2 当前 Web 的 exact gate

**BUNDLE：** `1ow-45tja2vjr.js`：

- byte 26,820 / 32,704 读取 `clip.is_download_unlocked`；只有严格等于 `true`
  才视为已解锁；
- byte 28,208 调用：

```http
POST /api/download/authorize
Content-Type: application/json

{"item_id":"<clip id>","item_type":"clip"}
```

- Web 检查 `response.data.ok`；失败时读取
  `reason ?? "authorization_failed"` 和 `message`；
- byte 28,597 读取 `credit_deducted`；为 true 时重新加载 subscription/billing；
- 授权成功后标记该 clip 已解锁，再对用户选择的每个格式依次请求：

```http
GET /api/download/clip/{clip_id}?format=<format>
```

独立 handler 链确认当前限制流的 exact format 为：

```text
MP3   -> GET /api/download/clip/{id}?format=mp3
M4A   -> GET /api/download/clip/{id}?format=m4a
WAV   -> GET /api/download/clip/{id}?format=wav
Video -> GET /api/download/clip/{id}?format=mp4
```

旧的 `GET /api/gen/{id}/wav_file/`、`POST /api/gen/{id}/convert_wav/` 与直接
`clip.video_url` fallback 仍存在于 legacy/Stems 流程，所以当前不是“删除旧 route”的全局
硬切换。兼容这些路径的前提必须是 parent/source clip 已严格解锁或本次 authorize 成功；
authorize 失败、结果不明或 read-only 禁止授权时，绝不能 fallback 到旧路径绕过额度门控。
当前 Web 没有 OPUS 格式项，完整 `/create` 闭包也未出现 OPUS route 字符串；CLI 可保留
历史兼容，但必须同样先确认 source 解锁，并把它标为 Web-current 未验证能力。

这里确认的是 clip 级解锁：同一个授权动作后可以处理多个被选择的格式。不能把授权放进
“每个格式下载”循环，否则一次 MP3 + WAV 选择可能错误地授权两次。

Stems 的 current gate 使用 parent clip：Pro 未解锁时以 parent ID 打开同一个
`DOWNLOAD_RESTRICTIONS` authorize-only 流程，成功后 stem ZIP 和单 stem 共用该 source
解锁，而不是逐个 stem clip 扣次。官方
[`Upcoming Changes FAQ`](https://help.suno.com/en/articles/13614785) 同样说明一首歌及其全部
stems 合计一次下载。

当前 Web 只消费以下 authorize response 字段：

```json
{
  "ok": true,
  "reason": "<optional string>",
  "message": "<optional string>",
  "credit_deducted": true
}
```

这只是**已捕获字段集合**，不是声称服务端响应只能有四个字段。没有调用真实 authorize，
因此本报告不猜测 HTTP error envelope、幂等 key、具体 allowance/top-up 扣减顺序或额外响应
字段。

### 1.3 billing exact fields

同一 current bundle byte 32,684～33,318 的额度计算为：

```text
period_remaining = max(0,
    current_period_downloads_limit - current_period_downloads_used)

downloads_remaining = period_remaining + additional_download_remaining
```

对应 response 投影：

```json
{
  "download_usage": {
    "current_period_downloads_limit": 20,
    "current_period_downloads_used": 0,
    "additional_download_remaining": 0
  },
  "download_credit_packs": []
}
```

数值仅用于展示 shape；实际值必须以账号 billing 返回为准。bundle byte 6,155 还读取
`download_credit_packs` 来展示额外下载包。另一条 current bundle 链确认每个 pack 至少读取
`id`、`amount`、`price_amount`、`price_currency_code`，并兼容 `price_usd`。购买链使用：

```http
POST /api/billing/purchase-credits/
{"id":"<pack id>","amount":<amount>,"checkout_success_path":"...","checkout_cancel_path":"..."}

GET /api/billing/purchase-credits/{item_id}/
```

它消费 `status`（`paid|failed|void|requires_action`）、`checkout_url` 与
`verification_redirect_url`。本轮没有触发购买，也不建议 CLI 默认增加付款操作；眼下只需
类型化并展示 quota/packs，保留未知字段。若未来要实现购买，必须作为独立、显式确认的付费
工作流审计和验收。

### 1.4 CLI 必须具备的失败语义

1. 先读取 clip 的 `is_download_unlocked`。值缺失、null 或 false 都不能当成 true。
2. 已解锁 clip 可以跳过 authorize；未解锁 clip 必须先 authorize。
3. `ok != true` 时停止文件 GET，向用户保留 `reason` 与 `message`。
4. `credit_deducted == true` 时刷新 billing；false/缺失不能被解释成“授权失败”。
5. authorize 是可能计量/扣额度的写请求。超时、连接中断或无法确认响应时，默认不得自动
   重试；应先重读 clip/billing 判断是否已经解锁/扣减。
6. 批量任务按唯一 clip 授权；同一 clip 的多个格式共享一次解锁。
7. read-only 模式遇到未解锁 clip 应 fail closed，不能偷偷 POST authorize；即使已解锁，
   是否允许实际文件 GET 仍应遵循 CLI 对 read-only 的既有定义。
8. 不要按套餐名字硬编码 20/60；所有 quota 显示与 gate 均使用 billing 当前字段。官方
   数字是政策说明，服务端字段才是运行时事实。

## 2. V6 已投放营销/UI，但 generation contract 尚未发布完整

### 已确认

- 登录态 `/create` HTML byte 113,543 包含 localization key
  `unlimited_v6_promo_web`，byte 113,586 的标题为 `V6 is here.`；
- `22xm7pc-z1o96.js` byte 885 使用
  `GET /api/cms/create/info-chip`，读取 `content.countdown.timestamp`；没有 timestamp
  就不展示倒计时；
- 这说明 V6 宣传/灰度 UI 已经部署到 create 页面。

### 未确认，不能猜

- 官方 [`Suno Release Notes`](https://suno.com/release-notes) 在本次抓取时最新公开项仍是
  2026-08-20 的 “Offline Playlists on Mobile”，没有 V6 release note；
- 当前 Pro billing 回读的 generation model 仍只有 v5.5、v5、v4.5+、v4.5、
  v4.5-all、v4、v3.5、v3、v2；默认仍是 v5.5 / `chirp-fenix`；
- 没有回读到名称 V6 的 model、V6 external key、V6 capabilities、V6 max lengths 或
  V6 generation response shape；
- 当前 bundle 的 generation tier 映射仍以 V5_5 / fenix 为最高已确认层级，未找到可用的
  `ModelTier.V6` request 映射；
- `15yl1sqicp-3l.js` 的 `DEFAULT_GOOSE_MODEL_NAME="chirp-goose"` 与 Create Speech/
  instrument seed 相关代码在 8 月 23 日 immutable bundle 已存在，不能把它当成本周新增
  或证明它等于 V6；
- 主页的 `Aura-1` 是营销图片/资产文字，没有 request builder 证据，不能作为模型 key。

因此当前正确做法是：展示“检测到 V6 promo，但账号 API 尚未返回可用模型”；继续动态读取
billing。等 Pro billing 实际返回新模型时，再保存其 `external_key`、capabilities、features、
allowed condition combinations、limits 与默认状态，并做一次有明确授权的最小私有生成闭环。

## 3. 旧模型退役是近期兼容风险，不是当前可猜的 route 迁移

同一份 Suno 官方 8 月 10 日公告称，新一代 models 上线时 **all prior models will be
retired**；旧作品仍留在 library，可播放、分享并用于 cover/remix。

对 CLI 的含义：

- 不应把 `chirp-fenix` 或更旧 key 当作永远存在的默认值；
- 用户未显式选模型时，以 billing 的 `is_default_model` 为准；
- 用户显式选择一个已不在 billing 的旧 model 时，应明确提示“账号当前不再提供该模型”，
  不要静默换到新模型，因为输出与扣费语义可能变化；
- Extend、Inspire、Cover、Persona/Voice 等非普通 generate 也必须根据新 model 的
  capabilities/allowed combinations 重新校验；
- V6 上线后需要重新抓取 request builder、response、task matrix 与 max lengths。旧模型
  退役公告没有证明 generation endpoint 一定改变。

仓库当前还有一个需要在模型切换前删除的静态逃生路径：
`src/api/generate.rs` 在 billing 读取发生 transport error、请求模型为 `auto` 且未使用
duration/特殊 task/feature 时，会把 `mv` 改写为固定 `chirp-auk-turbo` 后继续提交。该行为
今天仍可能成功，但与“旧模型全部退役”直接冲突，也与 CLI 其他显式 selector 的 fail-closed
策略不一致。正确迁移是 billing 不可验证时停止在 mutation 前，不能拿一个旧 free model
绕过动态模型发现；相应 endpoint test 也应从“发送 fallback model”改为“generation POST 为
零次”。

截至本次抓取，当前 generation submit 仍在 `33uyx1goiop-7.js` byte 17,357 调用：

```http
POST /api/generate/v2-web/
```

所以此刻不迁移 endpoint，只增强动态模型兼容与退役报错。

## 4. 当前 Pro 账号只读验收

2026-08-30 02:02 Asia/Shanghai 使用已配置账号执行 billing/capabilities 只读回读：

- plan：`Pro Plan` / `pro`；active；yearly；
- generation 默认：v5.5 / `chirp-fenix`；
- generation 可用列表：v5.5、v5、v4.5+、v4.5、v4.5-all、v4、v3.5、v3、v2；
- v5.5 当前 limits：prompt 5000、tags 1000、negative tags 1000、GPT description
  3000、title 100；本次 response 没有 duration 字段，不能把缺失值硬补成常量；
- Remaster selector：v5.5 / `chirp-flounder`（default）、v5 / `chirp-carp`、
  v4.5+ / `chirp-bass`；
- billing 顶层已经出现 `download_credit_packs: []`，但尚未返回 `download_usage`；
- 最近 5 个 feed clip 及其中一个 exact clip detail 尚未返回 `is_download_unlocked`；current
  Web 对缺失值按“未解锁”处理，而不是按兼容性默认 true；
- 没有 V6 model 回读。

这证明当前账号仍是有效 Pro，现有 v5.5 与 Remaster 动态能力可读；不证明一次未执行的
Remaster 或 V6 generation 会成功，也不证明下载 authorize 的实际扣减顺序。

本轮没有为了“验证协议”调用可能扣费的接口。下载 P0 的 exact shape 已由当前一方 bundle
与官方生效公告交叉确认，没必要用账号额度做探针。

## 5. 其他协议族复核

### Generation 与 Cowrite

当前 `/api/generate/v2-web/` 未见 v3 route 替换。`3od223yrej4sl.js` 的 builder 仍保留
`metadata.web_client_pathname`、`create_session_token`、`transaction_uuid`、
token provider、control sliders 与现有 task 投影。

完整 Cowrite lazy chunk `0td1i5ua_mp61.js` byte 8,732 仍调用：

```http
POST /api/generate/cowrite-lyrics/
```

body 仍包含 `selected`、`context_before`、`context_after`、`instruction`、`title`、
`style`、`mode`、`references`、`num_variants`、`lyricist_id`、
`metadata.lyrics_model`、`metadata.enable_thinking`、`create_session_token` 与
`lyrics_project_id`；response 继续消费 `edited_lyrics`、`lyrics_request_id`、
`lyrics_id`、`variants`、`artist_to_tag_mapping`、`next_prompts`。同一 chunk byte
18,581 仍保留 `/api/generate/rhymes/`。

另一个 focused editor chunk 使用较小 body/无尾斜杠调用是独立的局部编辑操作，不是完整
Cowrite contract 被替换。现有 CLI Cowrite 不需要迁移。

### Cover Art / `SONG_COVER_ART`

当前完整依赖闭包仍包含以下 route：

```text
GET  /api/video_gen/model-configs
POST /api/video_gen/cost/image
POST /api/video_gen/cost/video
POST /api/video_gen/image/generate
POST /api/video_gen/video/generate
POST /api/video_gen/pending_batches
POST /api/video_gen/poll_batches
POST /api/video_gen/history
GET  /api/video_gen/image/batch/{batch_id}
```

主证据分片包括：

- `0kt4cgxw46qxv.js`, SHA-256
  `6b73ef9401b0574e123bc3c91f07b3d7d34f0783b9d989ef006a887f881c2238`：
  model-configs、image/video generate；
- `0svt9svf0qvvu.js`, SHA-256
  `2698334a01eed8c2a13b8864576540029ae55cafbca815003dfe5a0f6435e27c`：
  cost、history、poll；
- `0l2dlsstnsjj5.js`, SHA-256
  `f9abf6ee9621052a479a02e020b11289fe0cb2dfc85fe320c707dcdae916944d`：
  pending/poll。

没有观察到 `/v2` route 或新的 submit endpoint 来替换 8 月 24 已确认的批量协议。因此现有
动态 model category、duration、cost、batch IDs、poll/history 与按生成 media ID apply 的
实现继续成立。对 bundle 中未由 request builder 捕获的新字段不做猜测；若服务端后续把
V6 图像能力并入 Cover Art，应先从 `model-configs` 动态暴露，而不是硬编码 category。

### Staff preview 与 forbidden sentinel

`3od223yrej4sl.js` byte 1,510,660 新出现：

```http
POST /api/gen/{gen_id}/unlock-preview
```

调用模块名和 toast namespace 都是 `staffTools`；成功后加入 preview polling。它不是普通
用户 clip 下载解锁，也不能替代 `/api/download/authorize`。

同一分片 byte 1,511,526 把 pathname `/api/forbidden` 当作不可用 audio URL，返回 null。
它是 media URL sentinel，不是应由 CLI 主动请求的新 API。

### 新增但不要求迁移的能力

当前 closure 还确认了以下增量接口；它们没有替换 CLI 已调用的 route，可按产品价值另行补齐：

- Lyrics rhymes：`POST /api/generate/rhymes/`；
- Remaster 仍走 `/api/generate/upsample`，但 body 新增可选
  `freedom`、`tone`、`strength`、`clarity`、`stereo_width`、`tags`、
  `variation_category`、`style_profile`；
- existing-clip Voice/Vox：`POST /api/clip/{clip_id}/vox-stem`，上传/录音 Voice 的
  `/api/processed_clip/voice-vox-stem` 未变化；
- Stems/MIDI：`GET /api/gen/{clip_id}/key`、`/midi`；
- Cover Art 管理：`/api/video_gen/action`、`/delete`、`/favorite`、`/favorites` 与
  `/api/video_gen/media/{media_type}/{media_id}/download`；
- 下载辅助：`/api/download/clips/zip/prepare`、`/api/download/sample-pack/{id}`。

相反，OPUS route 与 persona 的部分 paginated/visibility route 只是在当前 `/create` closure
中未出现，不能据此证明服务端删除；应保留兼容并针对对应页面或 live GET 单独确认。

### 与本次协议变化分开的现有安全债

只读仓库复核还发现，部分早期非幂等写仍通过通用 `with_auth_retry` 在显式 401 后刷新 JWT
并重放，而新版 Voice、Custom Model、Lyrics Projects、Cover Art 已采用“不自动重放写”的
更严格模式。另有 clip/persona/playlist 的部分 trash、visibility、reaction、reorder 等写在
2xx 后没有逐字段业务 readback。它们不是 8 月 30 日 bundle 证明的 route 失效，但属于下一版
应统一的 P1/P2 安全项：写前刷新认证，写请求最多一次；丢响应/5xx 返回可恢复的 ambiguity；
可读取的资源在成功后做 exact ID/state/field readback。

## 6. 迁移验收清单

### P0 下载

- [x] Clip decode 保留 `is_download_unlocked: Option<bool>`，缺失不等于 true；
- [x] Billing decode 保留三个 `download_usage` 数值字段及未知字段；
- [x] Billing decode 保留 `download_credit_packs` 的已确认字段和未知字段；付款 route 仅记录，
      不在本次默认实现范围内；
- [x] 未解锁 clip 在任何格式 GET 前只 authorize 一次；
- [x] MP3/M4A/WAV/video 主流程分别使用 prepared `mp3|m4a|wav|mp4`；
- [x] legacy WAV/direct video/OPUS fallback 只能在 source 已解锁后运行；
- [x] Stems 用 parent source clip 解锁一次，不能逐 stem 授权；
- [x] `ok/reason/message/credit_deducted` 全部保留并展示正确；
- [x] `ok != true` 不继续下载；
- [x] `credit_deducted == true` 后重读 billing；
- [x] authorize 不做盲目自动重试；歧义结果先 readback；
- [x] authorize transport 不跟随 redirect，避免 307/308 隐式重放 POST；返回的 3xx 按
      ambiguity 处理并进入 clip/billing readback；
- [x] 批量目标路径冲突在 authorize 前整体失败，`--force` 不绕过该检查；
- [x] `capabilities` 和歧义恢复证据只投影已确认 billing 字段；未知字段仅由原始
      `credits --json` 保留；
- [x] 多格式、多 clip、部分失败、已解锁、无额度、read-only 均有测试；
- [x] 旧服务端暂未返回新字段时 fail closed，而不是绕过 authorize；
- [x] 文档明确 Pro 9 月 3 日起为官方所述 20/月，但运行时不硬编码这个值。

### P1 模型退役

- [x] 所有生成入口共用当前 billing model resolver；
- [x] 删除 billing transport failure 时的固定 `chirp-auk-turbo` auto fallback；
- [x] 默认模型来自服务端，显式旧 selector 缺失时明确报错；
- [x] capability/task/limits 在提交前按选定模型重新验证；
- [x] agent-info 不把静态旧模型列表描述为“账号当前可用”；
- [ ] V6 出现在 Pro billing 后再补 alias、help 与协议 fixture。

实现完成后再次使用同一 Pro 账号执行了 `capabilities` 与 `credits` 只读验收：响应仍为
active Pro/year，`download_credit_packs=[]`，`download_usage` 尚未下发；新类型和缺失字段兼容
均通过，过程中没有调用 authorize、generation 或其他账号业务写接口。Rust 全量测试、
Clippy 和格式校验结果记录在对应提交中。

## 最终判断

“所有接口都已适配最新协议”在 2026-08-30 已不能无条件成立：**下载授权/额度是一个已确认
且有 9 月 3 日 deadline 的 P0 变化。** 这一项修完并做回归后，现有 generation、Cowrite、
Cover Art、Remaster 等主协议在当前部署上没有发现必须迁移的新版本。

V6 处于“营销/UI 已上线、账号 generation schema 尚未暴露”的过渡状态。此时最安全的
最新协议适配不是猜 key，而是保持 billing 驱动，并在官方 release note 与 Pro billing
真正给出 V6 contract 后立即做一次新的 source + live-read 审计。
