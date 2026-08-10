---
name: itsm
description: >
  ITSM 工单查询与操作能力（itsm-tools MCP）。用户提到工单、itsm、ITSM、incident，或给出 IM 开头的
  工单号（如 IM26070065）时使用。覆盖：按工单号查详情与历史回复、按工单号/主题/客户组搜索工单、
  回复工单、解决(Resolved)、暂挂、解挂，以及代提补单（按服务目录/客户组/提单人新建工单）。
  触发词：工单、itsm、ITSM、IM 开头单号、incident、查工单、工单详情、工单回复、回复工单、
  解决工单、关闭工单、暂挂、挂起、解挂、客户组工单、补单、代提、建单、提单、创建工单。
  即使用户只贴一个 IM 开头的编号，也应识别为 ITSM 工单并触发。
---

# ITSM 工单（itsm-tools）

itsm-tools MCP server，16 个工具分**读取**与**写入**两类。写入类（含补单建单）会改动或新建真实线上工单。

## 关键概念：两套 ID（最易踩坑）

ITSM 工单有两套标识，工具参数要求不同，混用会查不到或改错单：

| 标识 | 长什么样 | 哪些工具用 |
|------|----------|-----------|
| incidentCode（展示单号） | `IM` 开头，如 `IM26070065` | `get_ticket_by_code(code=…)` |
| incidentId（内部 ID） | 非 IM 前缀的字符串 | `get_detail` / `list_replies` / `reply` / `resolve` / `suspend` / `unhang` 的 id / order_id |

**用户给的几乎都是 incidentCode（IM 开头）。** 写操作需要的 incidentId 可由 `get_ticket_by_code`
一步取得（它同时返回详情、历史回复和 incidentId），所以"已知 IM 单号"时它是首选入口。

## 识别工单号

- `IM` + 数字（如 `IM26070065`、`IM25123456`）→ 大概率是 ITSM 工单号（incidentCode），
  直接用 `get_ticket_by_code` 查。
- 其他前缀 / 纯数字 / 不确定 → 不轻易假定，先跟用户确认是不是工单号再调，尤其不盲目调写操作。

## 工具选择（按意图路由）

### 读取类（无副作用，可直接调用）

| 意图 | 工具 | 入参 |
|------|------|------|
| 给了 IM 单号，想看详情（+历史回复） | `get_ticket_by_code` | code（首选，一步到位，顺带拿 incidentId） |
| 已有 incidentId，只要精简详情 | `get_detail` | id |
| 看历史回复轨迹 | `list_replies` | incident_id（或直接用 get_ticket_by_code） |
| 按工单号 / 主题关键字搜 | `search_tickets_by_code` | keyword（模糊匹配 codeAndSubject） |
| 按客户组名称搜 | `search_tickets_by_customer_group` | keyword（模糊匹配 contactCustomerGroupName） |
| 不知道用哪个视图 / 要换视图 | `list_views` | 无参，返回各视图 seachType |
| **补单前置：列三级服务目录树** | `list_service_tree` | 无参；取二级 stId 作 service_type、三级叶子 stId 作 service_sub_type |
| **补单前置：按服务叶子取补单模板** | `get_replenish_template` | leaf_id（= service_sub_type）；返回 data.id 作 create_template_id |
| **补单前置：搜客户组** | `search_customer_groups` | keyword；取 cgId + customerGroupName |
| **补单前置：搜人员（提单/支持人）** | `search_base_persons` | keyword；取 userId + psnName |
| **补单前置：列支持组** | `list_support_groups` | 无参；取 sgId + supportGroupName（可省略走应用默认） |

搜索类带可选 `seach_type`（视图 seachType），省略时用设置中的 MCP 缺省视图（默认 7=所有工单）；
可先 `list_views` 取可用视图。结果分页：`page_index`（从 1 起）、`page_size`（1..200，默认 50）。
`get_ticket_by_code` 命中多条时取首条并附 hint，需提示用户可能有多条。

### 写入类（修改真实 ITSM 工单，调用前必须向用户确认）

| 意图 | 工具 | 关键入参 |
|------|------|---------|
| 回复工单（追加回复，不改状态） | `reply` | order_id(incidentId)、detail(建议 HTML)、is_private(true=内部备注 / false=公开) |
| 解决 / 关闭工单（状态→Resolved） | `resolve` | id(incidentId)、solution(不能为空) |
| 暂挂工单 | `suspend` | id(incidentId)、reason |
| 解除暂挂 | `unhang` | id(incidentId) |
| **补单：代提新工单** | `create_ticket` | service_type、service_sub_type、order_subject、detail(HTML)、contact_customer_group(+_name)、requestor(+_name)；可选 create_template_id、assign(+_name)、support_by(+_name) |

> 这些操作直接改动线上工单，属外向、难回滚操作。按全局规则，未获用户明确授权前先确认
> （把工单号、动作、内容/原因摆出来再调）。`reply` 的 `order_type` 默认 `"1"`（与工单 orderType
> 一致），无需自行指定；首版不上传附件，fileIds 固定空。`detail` 建议用 HTML 保留排版
> （换行 `<br>`、段落 `<p>`、重点 `<strong>`、列表 `<ul>/<li>`），不要依赖纯文本换行。

## 补单（代提新工单）

补单 = 代他人新建一张线上 ITSM 工单，是**最重的写操作**（直接新建真实工单）。建单前必须把"给谁提、
什么服务、主题、详情、客户组、支持组/人"列清楚，获用户明确确认后再调。补单是多步链路，依赖 5 个
前置读取工具，无法一步到位：

1. `list_service_tree()` → 取**二级** stId 作 `service_type`、**三级叶子** stId 作 `service_sub_type`
2. `get_replenish_template(leaf_id=service_sub_type)` → 取返回 `data.id` 作 `create_template_id`
3. `search_customer_groups(keyword=客户组名)` → 取 `cgId` + `customerGroupName`
   作 `contact_customer_group` + `contact_customer_group_name`
4. `search_base_persons(keyword=姓名)` → 取 `userId` + `psnName`
   作 `requestor` + `requestor_name`；同理取可选的 `support_by` + `support_name`
5. `create_ticket(...)` 建单，`code==800` 时 `data` 为新单 **incidentId**（内部 ID，非 IM 单号）
6. `get_detail(id=新 incidentId)` 取展示单号 **incidentCode**（IM 开头）回报给用户

**ID 必须配套名称一起传**（否则后端校验失败）：`contact_customer_group`+`contact_customer_group_name`、
`requestor`+`requestor_name`、`support_by`+`support_name`、`assign`+`assign_name`。
`assign`（支持组）省略时走应用设置中的默认支持组；`support_by`（支持人）可省略。
`detail` 同 reply，用 HTML 保留排版（`<br>`/`<p>`/`<strong>`/`<ul><li>`/`<table>`）。

## 调用流程

1. schema 默认 deferred。先 `ToolSearch query="select:mcp__itsm-tools__<tool_name>"` 加载目标工具。
2. 识别意图 + 标识类型（IM 单号 = incidentCode；其他多为 incidentId）。
3. 读取类直接调；写入类先把"对哪张单、做什么、内容/原因是什么"列给用户，确认后再调。
4. 解析返回作答；搜索类注意 count 与分页，结果多时分页或收窄关键字。

## 典型链路

- "看看 IM26070065" → `get_ticket_by_code(code="IM26070065")`
- "IM26070065 的回复记录" → `get_ticket_by_code` 已含回复；或先取 incidentId 再 `list_replies`
- "回复 IM26070065：xxx" → 先 `get_ticket_by_code` 拿 incidentId → 确认 → `reply(order_id=…, detail=…, is_private=…)`
- "把 IM26070065 解决了，方案是 xxx" → 取 incidentId → 确认 → `resolve(id=…, solution=…)`
- "搜一下含'登录超时'的工单" → `search_tickets_by_code(keyword="登录超时")`
- "XX 客户组有哪些工单" → `search_tickets_by_customer_group(keyword="XX")`
- "我能用哪些工单视图" → `list_views()`
- "帮我代提一张单：服务目录 X>Y>Z，客户组 A，提单人 B，主题 C，详情 D" →
  `list_service_tree` → `get_replenish_template` → `search_customer_groups` → `search_base_persons`
  → 列清单确认 → `create_ticket(...)` → `get_detail` 取 IM 单号回报

## 边界

- 只处理 ITSM 工单相关意图。非工单的内部任务、代码问题不触发。
- IM 前缀只是"大概率"；遇到不确定的编号先确认是不是工单号，绝不盲目调写操作。
- 写操作未获明确授权不执行；用户只说"看看 / 查查"绝不触发 `reply` / `resolve` / `suspend` / `unhang` / `create_ticket`。
- 补单（`create_ticket`）会新建真实工单，比回复/解决更重；未列清单确认不建单。返回的是内部 incidentId，
  需再 `get_detail` 换成 IM 单号才能给用户看。
- 调用失败 / 无结果 / 权限不足 → 如实报告状态，不编造工单内容或字段。
- 多环境/多账号下视图与权限可能不同，以 `list_views` 实时返回为准，不缓存、不假设。
