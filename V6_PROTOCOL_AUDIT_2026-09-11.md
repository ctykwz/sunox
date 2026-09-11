# Sunox / Suno Web v6 协议审计 — 2026-09-11

## 结论

截至 2026-09-11，当前账号的只读 `/api/billing/info/` 已正式返回三种 v6
generation model 和一个 v6 Remaster model：

| 展示名 | generation external key | 当前账号 | 定位 |
|---|---|---|---|
| `v6` | `chirp-hawk` | `can_use=true`、默认、Pro badge | 主力、通用、精修版 |
| `v6-wild` | `chirp-hawk-wild` | `can_use=true`、Pro badge | 实验性创意 |
| `v6-mini` | `chirp-goose` | `can_use=true`、无 Pro badge | 免费、效率优先版本 |
| `v6` Remaster | `chirp-halibut` | 单独位于 `remaster_model_types`、默认；legacy `can_use=false` | 新 Remaster 请求模型 |

三个 generation model 都声明 `all` 能力（主 v6 另有 `underpainting`），都声明
`create_control_sliders`、`tag_upsample`、`mumble_mode`、`vox_and_voices`、
`reuse_styles_lyrics`，并给出相同的 `allowed_condition_combinations`。因此 v6 迭代不只是
替换默认模型：Web 当前还增加或明确化了 Mumble、复用 Styles/Lyrics、Max Mode、
Underpaint/Overpaint、v6 Remaster 完整参数和一批 v2-web 可选字段。

最重要的实施结论：

1. **P0：修复 v6 `--duration` 兼容性。** 当前 Web 对 v6 Custom generation 接受
   10～360 秒并把它作为 top-level `duration` 发送；Sunox 已移除仅允许
   `chirp-fenix` 的旧限制。v6 description 虽能被服务端接受 duration，但真实结果不遵从，
   因而仍按当前 Web 行为拒绝该参数。
2. **P0：所有模型和 condition 必须继续按实时 billing 解析。** 不应从名称猜测
   `v6-wild` 或把 `chirp-goose` 继续当作未命名内部模型。
3. **P1：补齐 v6 Variety、Mumble 与 Max Mode。** Variety 是独立的
   `metadata.control_sliders.aug_creativity`，不是现有 Weirdness，并受 `aug-creativity` Web gate 控制；Mumble 同时受 Web gate 和
   model 支持约束；Max Mode 同时受账号 `max_mode` entitlement、feature gate 和模型支持约束。
4. **P1：按当前 v2 modal 收紧 v6 Remaster。** variation 为
   `subtle|normal|high`（默认 `normal`），style profile 为
   `natural|boost|clarity`（默认 `boost`）；真实写入确认显式值可端到端保留。底层 builder
   虽保留 tags 和五个 slider，但当前 modal 不展示，真实账号也未证明普通用户可用，因此 CLI 不开放。
5. **P2：实现 Underpaint/Overpaint 时必须区分三套名称。** UI condition 是
   `underpaint/overpaint`，generation task 是 `underpainting/overpainting`，body 是
   `underpainting_clip_id/overpainting_clip_id`。
6. **P2：不要把 `reuse_styles_lyrics` 作为后端 task 直接提交。** Web 将源 clip 的歌词/风格
   折叠进 Onebox 的 override，再从 references/stacked task 中移除该条件。

本轮落地状态：P0 已完成；P1 已实现 v6 duration、Variety（含 Web 默认值）、Mumble、
Max Mode，以及 v6 Remaster 已确认的 variation/style profile 枚举与默认值。Mumble/Max Mode
采用显式 CLI opt-in，并在写入前同时校验实时 billing/model capability 和 `/api/session/`
的 `aug-creativity` / `mumble-mode` / `max-mode` flag；缺少 Variety gate 时不会注入默认值。
P2 已实现 Reuse Styles/Lyrics 的客户端展开，以及带实时权限、ownership、源 eligibility 和模型
condition 前置校验的 Underpaint/Overpaint；Studio/stacked workflows 仍未实现。

### 当前账号复核与真实写验证

2026-09-11 先使用当前已配置账号执行 `--read-only` 的 `doctor`、`models`、`credits`、
`capabilities` 和匿名 Clip 汇总，再按用户授权串行完成最小真实写验证：

- 账号为有效 Pro 年付套餐，本机配置和账号默认模型均为 `chirp-hawk`；
- `chirp-hawk`、`chirp-hawk-wild`、`chirp-goose` 均为 `can_use=true`，三个模型均实时声明
  `reuse_styles_lyrics`、`mumble_mode`、control sliders，以及单项 `underpaint` / `overpaint`
  condition；账号同时声明 `edit_mode`、`max_mode`、`remaster`；
- 匿名抽样最近 100 个 Clip：100 个 complete，99 个 generation、1 个 upload；100 个具备
  Reuse 所需 lyrics/styles 形态，47 个符合 Overpaint 静态形态，1 个符合 Underpaint 静态形态。
  单独按 upload 过滤可读到 8 个 complete、未删除且有 owner 的上传素材；
- 最近 100 个本人 Clip 均返回 enabled `remix_reuse_style` 和 `remaster` action，但没有返回
  `add_instrumental` / `add_vocal` action。Paint 的可用性由 Create 表单的独立 helper、账号
  entitlement 和模型 condition 决定，因此不把缺失的 feed action 当作拒绝条件。Action 是 Web
  菜单可见性证据，不足以证明公开作品不能 Reuse，因此暂不将 ownership/action 误设为 Reuse
  的硬门槛；Underpaint/Overpaint 仍按已确认的仅本人素材规则严格校验 ownership；
- JWT 的 Clerk `sub` 不等于 Clip owner；`suno.com/claims/user_id` 与 Clip `user_id` 精确匹配。
  因此资源 ownership 使用该 Suno claim（保留 legacy fallback），账号锁与登录一致性仍使用 `sub`；
- v6 Pro Custom 成功，最终 `model_name=chirp-hawk`、`major_model_version=v6`，整数
  `aug_creativity=3` 被保留。先前小数 `0.37` 被服务端确定性 400 拒绝，提示必须为 0..4
  的整数；恢复检查确认该失败没有生成 Clip；
- Reuse 成功将源 lyrics/styles 客户端展开为普通 Custom 请求；Underpaint/Overpaint 成功保留
  `underpainting`/`overpainting` task、对应 source ID、`is_remix=true` 和 remix root；
- v6 Remaster 使用 `chirp-halibut` 成功；当前 v2 modal 确认默认发送
  `variation_category=normal`、`style_profile=boost`，显式 `high + clarity` 真实提交的两个结果均
  完成并在最终 Clip 元数据中保留；
- Mumble 的早期探测确实携带 `is_mumble=true`，但提交响应和最终 Clip 均回显 `false`。随后读取
  `/api/session/` 确认当前账号没有 `mumble-mode` flag；CLI 现会在提交前拒绝，不再消耗写请求；
- Max Mode 请求和最终 Clip 均保留 `is_max_mode=true`，默认 `aug_creativity=1`。同一请求指定
  `duration=10`，两个结果约为 9.6 秒和 168.88 秒，因此 Max Mode 下时长不是严格输出保证；
- description 生成前后 `total_credits_left` 从 2502 降至 2500，证明即使账号 session role 有
  `unlimited_credits=true`，也不能解释为所有写操作不计账；v6 Remaster 完成后余额又读到 2502，
  而 free-remaster 计数均为 0。该恢复可能是异步结算或退款，不能泛化为 Remaster 永远免费。

这次复核还发现并修正两项实现偏差：`can_buy_credit_top_ups` 曾因旧别名被误报为 unknown，
现按 account-only 能力报告；`chirp-halibut` 的 variation/style profile 现按当前 v2 modal 的
强类型枚举和默认值发送，不再使用自由字符串或未知值省略策略。

## 范围、安全边界与证据等级

本报告使用：

- 当前 Suno Web 的一方 `/create` HTML 及其可达 immutable bundle；
- 本机已有 `/private/tmp/sunox-v6-*` 抓取文件；
- 当前已配置账号的 `sunox models --json`、`sunox capabilities --json` 只读输出；
- 经用户明确授权、串行执行且完成只读回查的最小 v6 写请求；
- Suno 官网页面。

没有调用 upload、download authorization、metadata、playlist、reaction、delete 等无关写接口。
报告不记录账号、clip、batch、operation、transaction 或 session 标识。credits 只记录上述
短期相对变化，不记录账号标识，也不将余额恢复解释为免费。官网公开搜索截至本次审计未发现能替代 billing/bundle schema 的 v6 技术文档；因此
能力描述和请求结构以当前一方 API 回读及 Web bundle 为准，不从第三方资料推断。

证据标签：

- **CONFIRMED / LIVE-READ**：当前账号只读 API 实际返回；
- **CONFIRMED / LIVE-WRITE**：经授权的最小真实提交及最终 Clip 回读；
- **CONFIRMED / BUNDLE**：当前 Suno 一方 Web bundle 的请求构造或 gate；
- **INFERRED**：可由已确认的省略逻辑或组合逻辑可靠推导，但未发送写请求验证；
- **UNKNOWN**：当前只读响应和可达 bundle 没有给出，不能安全声称。

## 证据快照

登录态来源页对应 deploy `dpl_9TWeoWiCAYdER9oq7DRZmyFKgYMp`。压缩 JS 为单行，下面
使用原始文件 byte offset 定位。

| 文件 | SHA-256 | 用途 |
|---|---|---|
| `/private/tmp/sunox-v6-create.html` | `c27c2acf4b88e49818d6c3a00262b7232a8c233c0e7f286c351b3b5cb26e4e25` | 当前页面、deploy 与 chunk 入口 |
| `/private/tmp/sunox-v6-chunks/3fa1lxa4qd75n.js` | `3ce4e20e9514088787f756c29c5cc310fc0f2d6adec8fc68979f81e073ce0154` | v6 模型映射、generation builder、conditions、Mumble/Max gates |
| `/private/tmp/sunox-v6-upsample.js`（同 `25md-m585bpg0.js`） | `66e8271fb436f0e85465b39b18f876439b18abe45eb644ae496da770e11a0b89` | Remaster body builder |
| `/private/tmp/sunox-v6-remaster.js`（同 `265_w2c73vj-5.js`） | `8a61c178c1945f9d738212cd0563a74723af2fd036a2a394ec88c2bcb208aed6` | Remaster model feature helpers |
| `/private/tmp/sunox-v6-remaster-modal-chunks/0tcgu7vbznxyl.js` | `2c90b2332cd2e17377ff29daefb0abcd5c7a1d40eb8a12a9d6be250f9fe1e582` | Remaster 枚举与默认常量 |
| `/private/tmp/sunox-v6-remaster-modal-chunks/3mg1bpfz0zrv3.js` | `dbe99b433eb3543cab8caa517fee9a2f5752aa0b2b43b8e5a9bf181fdf0d0ecd` | 当前 v2 modal 的默认值与提交调用 |

immutable bundle URL 形如
`https://suno.com/_next/static/immutable/chunks/<chunk-name>`；例如上述主生成证据是
<https://suno.com/_next/static/immutable/chunks/3fa1lxa4qd75n.js>。

只读命令：

```text
target/debug/sunox models --json
target/debug/sunox capabilities --json
```

## 1. v6 / v6-wild / v6-mini

### 1.1 模型身份与产品定位

**CONFIRMED / LIVE-READ：** `models --json` 返回：

- `v6`：`chirp-hawk`，`major_version=6`，`is_default_model=true`，描述为
  “Powerful. Versatile. Refined. Our best model yet.”；
- `v6-wild`：`chirp-hawk-wild`，`major_version=6`，描述为
  “Best for experimental ideas.”；
- `v6-mini`：`chirp-goose`，`major_version=6`，描述为
  “A free, more efficient version of premium v6 models.”。

**CONFIRMED / BUNDLE：** `3fa1lxa4qd75n.js`：

- byte 1,909,541：`major_model_version == v6` 或 key 包含 `hawk` 映射为 V6；包含
  `goose` 映射为 V6-mini；
- byte 1,910,183 / 1,910,769：V6 与 V6-mini 分别映射为 `chirp-hawk`、
  `chirp-goose`；
- byte 1,235,138：`hawk-wild` 的默认 `aug_creativity` 为 `0`，普通 `hawk`/`goose`
  为 `1`。

`v6-wild -> chirp-hawk-wild` 的完整 selector 来自当前 LIVE-READ；bundle 单独确认 Web
认识 `hawk-wild`，两条证据互相吻合。

### 1.2 能力矩阵

**CONFIRMED / LIVE-READ：**

| 项目 | v6 | v6-wild | v6-mini |
|---|---:|---:|---:|
| `all` capability | 是 | 是 | 是 |
| `underpainting` capability | 是（额外显式声明） | 未单列 | 未单列 |
| `create_control_sliders` | 是 | 是 | 是 |
| `tag_upsample` | 是 | 是 | 是 |
| `mumble_mode` | 是 | 是 | 是 |
| `vox_and_voices` | 是 | 是 | 是 |
| `reuse_styles_lyrics` | 是 | 是 | 是 |
| Pro badge | 是 | 是 | 否 |
| 默认 model | 是 | 否 | 否 |

三者都允许同一组 combinations：`extend`、`cover`、`infill`、`persona`、
`persona+extend`、`persona+cover`、`playlist`、`underpaint`、`overpaint`、`vox`、
`vox+extend`、`vox+cover`、`vox+playlist`、`persona+infill`、`cover+infill`。

三者本次均返回相同上限：title 100、prompt 5000、tags 1000、negative_tags 1000、
gpt_description_prompt 3000。没有返回 duration 上限；不能由此断言服务端没有时长限制。

**INFERRED：** v6-wild 的 `aug_creativity` 默认 0，而另外两种 v6 默认 1，说明 Wild 的
实验性不是通过自动开启同一个 Variety 默认值实现的；CLI 不应把两个概念合并。

## 2. Mumble、Reuse Styles/Lyrics 与 Max Mode

### 2.1 Mumble Mode

**CONFIRMED / BUNDLE：** `3fa1lxa4qd75n.js`：

- byte 1,054,908：表单枚举值是 `mumble_mode`；
- byte 1,038,135：UI 还受 Statsig gate `mumble-mode` 和
  `modelValidForMumbleMode(model)` 双重约束；
- byte 1,914,439：网络字段是 `metadata.is_mumble`，模型不支持时为 `undefined`；
- byte 1,915,000 附近：Mumble 开启时，即使 lyrics 为空也不会把
  `make_instrumental` 自动设为 true；
- byte 1,172,390：复用已有 clip 时从 `metadata.is_mumble` 恢复表单状态。

**CONFIRMED / LIVE-READ：** 三个 v6 model 均在 `features` 中声明 `mumble_mode`。

**CONFIRMED / LIVE-WRITE：** 当前账号的 v6 Pro 请求确实发送 `metadata.is_mumble=true`，但
提交响应和最终两个 Clip 均回显 `is_mumble=false`。请求被接受不等于功能生效。

**CONFIRMED / LIVE-READ + BUNDLE：** 当前 Web 同时要求 `/api/session/` 的
`mumble-mode` flag；当前账号该 flag 缺失，与上述回显一致。Sunox 现会在发送前读取 session 并
fail closed。

结论：CLI 参数可叫 Mumble Mode，但请求不能发送 `mumble_mode`；当前协议是
`metadata.is_mumble=true`。必须同时满足选定模型 feature 和 session `mumble-mode` flag，并保持
`make_instrumental=false` 的语义；服务端回读仍是端到端结果的最终依据。

### 2.2 Reuse Styles & Lyrics

**CONFIRMED / BUNDLE：** `3fa1lxa4qd75n.js`：

- byte 1,054,927：condition 枚举为 `reuse_styles_lyrics`；
- byte 538,485、1,176,051 附近：读取源 clip 的 `metadata.prompt` 和 `metadata.tags`，
  写入 Onebox `overrideLyrics/overrideTags`；
- byte 538,485、1,170,960：在生成 references / stacked tasks 前显式过滤这个 condition；
- builder 随后将 overrides 落为 `prompt`、`tags` 和 `override_fields`。

**CONFIRMED / LIVE-READ：** 三个 v6 model 均声明 feature `reuse_styles_lyrics`。

结论：这是 UI reuse condition 和模型 feature，不是应直接提交的 generation `task`。
实现应先读取源内容、构造 overrides，再移除该伪 reference。

### 2.3 Max Mode

**CONFIRMED / BUNDLE：** `3fa1lxa4qd75n.js`：

- byte 1,038,776：显示条件同时要求账号 `isFeatureAllowed("max-mode")`、模型
  `modelSupportsMaxMode` 和套餐 `PlanFeature.MaxMode`；条件失效时会清掉已选状态；
- byte 1,162,536：模型支持函数认 `crow/eagle/fenix/goose/hawk` 或 custom model；
- byte 1,914,439：请求字段固定是 `metadata.is_max_mode`；
- 默认表单状态 `maxMode=false`。

**CONFIRMED / LIVE-READ：** 当前 Pro 账号的 plan/access features 包含 `max_mode`；这是一项
账号 entitlement，不在 generation model 的 `features` 数组中；`/api/session/` 同时返回
`max-mode=true`。

**CONFIRMED / LIVE-WRITE：** v6 Pro 提交及最终 Clip 均保留 `is_max_mode=true`，未显式传
Variety 时回显 `aug_creativity=1`。同一 `duration=10` 批次的两个结果约为 9.6 秒和
168.88 秒，因此 Max Mode 下请求时长不构成最终输出长度保证。

结论：不能仅凭 model name 或仅凭 plan feature 开启；应复刻 Web 的多层 gate。值是 bool，
当前关闭态请求为 `metadata.is_max_mode=false`。计费和最终输出时长仍以服务端为准。

## 3. Underpaint / Overpaint

**CONFIRMED / LIVE-READ：** 三个 v6 model 的 `allowed_condition_combinations` 都包含单项
`underpaint`、`overpaint`。

**CONFIRMED / BUNDLE：** `3fa1lxa4qd75n.js`：

| 层 | Underpaint | Overpaint |
|---|---|---|
| UI `ConditionTypes` | `underpaint` | `overpaint` |
| builder/reference task | `underpainting` | `overpainting` |
| request source field | `underpainting_clip_id` | `overpainting_clip_id` |

- byte 1,908,100～1,910,000：task resolver 选择 `underpainting` / `overpainting`；
- byte 1,915,000～1,919,232：写入对应 `*_clip_id`，并令
  `metadata.is_remix=true`；
- byte 1,907,643：两者不能彼此组合，也不能与 cover/persona/infill/extend/gen-stem/
  stem-condition/playlist 组合；
- byte 1,157,000 附近：Underpaint/Overpaint 只对自己的 clip 可见，并受 plan `edit_mode` 权限控制；
- bundle 导出的 `canAddInstrumental`：仅 upload 或 `Vocals`/`Backing_Vocals` stem；
- bundle 导出的 `canAddVocal`：仅 upload、`Instrumental` stem、空 prompt，或单行完整方括号 prompt；
- task 仍通过 `findModelForTaskType` 和 live model condition compatibility 选择。

**CONFIRMED / LIVE-WRITE：** Underpaint 与 Overpaint 均完成，最终 Clip 分别保留
`task=underpainting` / `task=overpainting`、正确 source ID、`is_remix=true` 和 remix root。
服务端仍是 eligibility、输出与计费的最终权威。

## 4. v6 Remaster 精确请求

### 4.1 路由与字段

**CONFIRMED / BUNDLE：** `/private/tmp/sunox-v6-upsample.js` byte 52,537～53,525：

```http
POST /api/generate/upsample
Content-Type: application/json

{
  "clip_id": "<source clip id>",
  "model_name": "chirp-halibut",
  "tags": "<non-empty trimmed style text>",
  "freedom": 0.0,
  "tone": 0.0,
  "strength": 0.0,
  "stereo_width": 0.0,
  "clarity": 0.0,
  "variation_category": "<truthy category>",
  "style_profile": "<truthy profile>"
}
```

这里只有 `clip_id` 无条件进入 body；其余字段都按下面规则有条件发送。示例中的数值不是
默认值，只用于表示 JSON number。

### 4.2 默认、省略、范围和 gate

| 字段 | Web 接受/归一化 | 当前默认/省略规则 | feature/model gate | 状态 |
|---|---|---|---|---|
| `model_name` | truthy string | 未选则省略 | 从独立 `remaster_model_types` 选择；v6=`chirp-halibut` | CONFIRMED |
| `tags` | builder 会透传非空值 | 当前 v2 modal 不展示；普通 Pro 真实请求返回 staff-only 400 | BUILDER-ONLY；CLI 不开放 |
| `freedom` | builder clamp 到 `[0,1]`，0 省略 | 当前 v2 modal 不展示；真实请求返回 Variation remaster is not allowed | BUILDER-ONLY；CLI 不开放 |
| `tone` | builder clamp 到 `[0,1]`，0.5 省略 | 当前 v2 modal 不展示；请求被接受但最终 metadata 不回显 | UNVERIFIED EFFECT；CLI 不开放 |
| `strength` | builder clamp 到 `[0,1]`，0.5 省略 | 当前 v2 modal 不展示；请求被接受但最终 metadata 不回显 | UNVERIFIED EFFECT；CLI 不开放 |
| `clarity` | builder clamp 到 `[0,1]`，0.5 省略 | 当前 v2 modal 不展示；请求被接受但最终 metadata 不回显 | UNVERIFIED EFFECT；CLI 不开放 |
| `stereo_width` | builder clamp 到 `[0,1]`，0.5 省略 | 当前 v2 modal 不展示；请求被接受但最终 metadata 不回显 | UNVERIFIED EFFECT；CLI 不开放 |
| `style_profile` | `natural|boost|clarity` | 默认 `boost`，v2 modal 会发送 | `supportsRemasterStyleProfile` 仅匹配 `chirp-halibut` | CONFIRMED / BUNDLE + LIVE-WRITE |
| `variation_category` | `subtle|normal|high` | 默认 `normal`，v2 modal 会发送 | strength/variation helper 包含 `chirp-halibut` | CONFIRMED / BUNDLE + LIVE-WRITE |

重要语义：builder 先 clamp 五个 slider，再用 truthiness/中点判断是否写字段。因此
`freedom=0` 与省略等价；另四个 slider 的 canonical 中性值是 `0.5`，此时省略。
`freedom` 的负值 clamp 到 0 后省略，而其余 slider 的 0 是有效、会发送的极值。

**CONFIRMED / BUNDLE：** `/private/tmp/sunox-v6-remaster.js` byte 24,132～24,700：

- `supportsRemasterStyleProfile(model)` 当前只检查 key 是否包含 `chirp-halibut`；
- `supportsRemasterVariationsStrength(model)` 接受 `chirp-carp`、`chirp-dorado`、
  `chirp-flounder`、`chirp-haddock`、`chirp-halibut`。

**CONFIRMED / LIVE-READ：** 当前账号 `remaster_model_types` 只返回：
`name=v6`、`external_key=chirp-halibut`、`is_default_model=true`、`can_use=false`；当前 Web
历史行为与仓库基线均表明 legacy `can_use` 不是菜单资格 gate，真正 gate 是 plan feature、
模型列表和源 clip action。

**CONFIRMED / LIVE-WRITE：** tags 的单变量请求得到确定性 staff-only 400；freedom 得到确定性
`Variation remaster is not allowed` 400。tone/strength/clarity/stereo_width 的单变量请求各自被
接受并完成两条结果，但最终 metadata 只回显默认 variation/style profile，没有回显 slider。所有
Remaster 探测完成后总余额恢复为探测前读数；这仍不证明字段生效或以后不计费。

当前 modal 常量与调用方已证明 variation/style profile 的枚举和默认值；真实 `high + clarity`
提交进一步证明了服务端接收和最终回显。其余 builder-only 控制均不作为公开 CLI 能力。

## 5. 其他生成协议变化

**CONFIRMED / BUNDLE：** generation endpoint 仍为
`POST /api/generate/v2-web/`（`1jw5qfhlrr3ch.js` byte 20,937），没有发现 v6 专用 route。
`3fa1lxa4qd75n.js` byte 1,913,000～1,922,516 的当前 builder 除旧字段外还可构造：

- `metadata.aug_creativity`：位于 `metadata.control_sliders`；受 `aug-creativity` Web gate；
  v6/v6-mini 默认 1，v6-wild 默认 0；
- `metadata.is_mumble`、`metadata.is_max_mode`；
- `metadata.create_surface`、`from_studio_project_id`、`disable_volume_normalization`、
  `batch_offset`、`recreated_from_clip_id`、`model_config`；
- `metadata.is_speech`、`backing_music`、`sound_configs`；
- top-level `duration`、`lyrics_project_id`、`lyricist_id`；
- personalization：`use_personalization`、`personalization_user_uuid`、
  `do_personalize_lyrics`；
- `midi_cond`、`audio_refs`；
- `user_uploaded_image_ids`、`user_uploaded_video_id`；
- agentic/attachment：`attached_lyrics`、`attached_styles`、`persona_voice_ref`；
- condition-specific：`underpainting_clip_id`、`overpainting_clip_id`、
  `metadata.stacked_task_clips` 等。

这些字段不是都“v6 新增”，而是当前 v6 时代 builder 的完整可见面。应按具体功能逐项实现，
不能为了 schema 完整而在普通生成中发送空字段。当前 builder 的重要模式是：可选字段大量用
`undefined`/truthy 条件省略，服务端 contract 应保持同样的缺省语义。

## 6. 对 Sunox 的优先级建议

### P0：正确性与未来兼容

1. 解除 `validate_generation_duration` 对 `chirp-fenix` 的硬编码限制，按当前 Web 的
   10～360 秒边界校验 v6 Custom generation，同时优先尊重 live billing 后续可能返回的
   `max_lengths.duration`。同步修正 CLI help、README 和 `agent-info` 的 v5.5-only 说明。
2. 保持每次写前实时读取 billing，按 exact external key / ID / 唯一展示名解析三种 v6；
   默认固定 `chirp-hawk` 后仍需验证 `can_use=true`，不要静态降级或猜测。
3. 所有新增写能力继续沿用现有“不可盲重试 + operation recovery”语义。

### P1：v6 可见能力

1. Variety 已按 Web 语义实现默认值：`chirp-hawk-wild=0`，
   `chirp-hawk`、`chirp-goose` 和 `major_version>=6` 为 1。它必须映射到
   `control_sliders.aug_creativity`，不能复用 Weirdness 参数。
2. Mumble 已在 Custom mode、model feature 和 session `mumble-mode` 校验后发送
   `metadata.is_mumble=true`，并阻止空歌词被改写为 instrumental；缺少 session flag 时提交前拒绝。
3. Max Mode 已检查账号 `max_mode` 和模型支持，并发送 `metadata.is_max_mode`；帮助中明确计费和
   最终时长由服务端决定。
4. Remaster 仅公开当前 v2 modal 已确认的 variation/style profile；`style_profile` 只对
   `chirp-halibut` 开放，并使用 `natural|boost|clarity` 强类型枚举和 `boost` 默认值。
   builder-only tags/sliders 不发送，避免 staff-only 拒绝或为未证明有效的字段消耗 credits。
5. `capabilities` 不应只汇总 account/plan feature；还应把 model-level
   `mumble_mode`、`reuse_styles_lyrics` 等能力纳入 CLI coverage，避免报告看起来“全支持”。

### P2：协议覆盖与观测性

1. Reuse Styles/Lyrics 的客户端展开已实现，未新增错误的 server task。
2. Underpaint/Overpaint 已对 condition combination、账号 Edit Mode、源 clip ownership/state/
   eligibility 和三套命名做强校验。
3. `capabilities --json` 已保留 raw `allowed_condition_combinations`、feature、
   `major_version` 与未知字段，便于下一次部署 diff。
4. 为当前 builder 中尚未映射的 personalization、MIDI、attachments、audio refs、
   `model_config` 分功能审计；没有用户入口或 read-only schema 时保持 unsupported。
5. 若未来 modal 正式重新暴露 tags/sliders，再按当时的 session/account gate、UI 范围和最终 readback
   重新接入；variation/style profile 已由 route-specific chunk 和真实 Remaster POST 确认。

## 7. Confirmed / Inferred / Unknown 汇总

| 项目 | 状态 | 最短结论 |
|---|---|---|
| 三种 v6 selector | CONFIRMED | hawk / hawk-wild / goose |
| 三种 v6 features 与 combinations | CONFIRMED | 实时 billing 已返回 |
| Variety 值域 | CONFIRMED / LIVE-WRITE | 0..4 整数；小数被确定性拒绝 |
| Mumble 请求 | CONFIRMED / LIVE-READ | model feature + session flag 双 gate；当前账号缺 flag，提交前拒绝 |
| Reuse Styles/Lyrics | CONFIRMED / LIVE-WRITE | 客户端 override，不是 server task |
| Max Mode 请求 | CONFIRMED / LIVE-WRITE | 可保留；最终时长不严格遵守请求值 |
| Underpaint/Overpaint 请求字段 | CONFIRMED / LIVE-WRITE | task、source、remix 均完成回读 |
| v6 Remaster model | CONFIRMED / LIVE-WRITE | `chirp-halibut` |
| Remaster builder-only tags/sliders | NOT EXPOSED | 当前 modal 不展示；tags/freedom 被拒绝，其余无最终回显 |
| style_profile 合法值/default | CONFIRMED / BUNDLE + LIVE-WRITE | natural/boost/clarity，默认 boost；clarity 最终回显 |
| v6 variation 完整值域/default | CONFIRMED / BUNDLE + LIVE-WRITE | subtle/normal/high，默认 normal；high 最终回显 |
| v6 新 endpoint | CONFIRMED 无变化 | 仍为 `/api/generate/v2-web/` |
| credits 结算 | PARTIAL / LIVE-READ | 生成后总余额短暂 -2，Remaster 完成后恢复；按操作/服务端结算，不推断免费 |
