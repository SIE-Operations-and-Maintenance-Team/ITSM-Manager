---
name: itsm-replenish
description: >
  ITSM 补单（代提新工单）全流程编排，专为非结构化输入设计。用户给出
  "XX项目XX人报告XXX内容，补个单"式的自然语言片段、或粘贴企业微信对话
  要求补单/代提/建单时使用。覆盖：解析原始输入到字段、并发反查服务目录/
  客户组/提单人、按内容定位三级问题类型、整理汇总详情、拼带来源标注的
  拟稿给用户确认、建单、取展示单号回报。触发词：补单、代提、建单、提单、
  创建工单、补个单、帮XX提张单、代XX提单。即使用户只贴一段企业微信对话
  说"补个单"，也触发。
---

# ITSM 补单流程（itsm-replenish）

把"XX项目XX人报告XXX内容"式的自然语言片段、或企业微信对话，端到端编排成一张
真实 ITSM 工单。补单是最重的写操作（建真实工单）——**建单前必须拼拟稿给用户确认**。

通用 ITSM 工单操作（查 / 回复 / 解决 / 暂挂）见 `itsm` skill；本 skill 专攻补单的深度编排。

## 输入特征与字段映射

用户输入通常是：

- **自然语言片段**：`XX项目XX人报告XXX内容，需要补个单`
- **企业微信对话**：多人多轮聊天记录

agent 第一步是从中解析出结构化字段。映射关系（agent 解析假设，拿不准时让用户纠正）：

| 输入里的信息 | → ITSM 字段 | 怎么得到 |
|---|---|---|
| XX项目 | `contact_customer_group` + `contact_customer_group_name` | `search_customer_groups(keyword=项目名)` |
| XX人（报告人） | `requestor` + `requestor_name` | `search_base_persons(keyword=人名)` |
| 内容 → 一句话 | `order_subject` | agent 从内容提炼 |
| 内容 → 正文 | `detail`（HTML） | 整理汇总原文转 HTML（见 detail 加工规范） |
| 服务目录 L1/L2 | —— | Config 默认（`default_service_l1/l2`，判断不了时） |
| 服务目录 L3（问题类型） | `service_sub_type` | agent 按内容判断（见服务目录策略） |
| 支持组 | `assign` + `assign_name` | Config 默认支持组（`default_support_group_id/name`） |
| 模板 | `create_template_id` | `get_replenish_template(leaf_id=L3)` 自动取 |

## 核心原则：乐观判断 + 混淆才确认

贯穿所有需要 agent 判断的字段（服务目录 L3、客户组匹配、提单人匹配）：

- **能判断就直接判断**，拟稿里带来源标注让用户核对
- **混淆 / 多结果才列候选问用户**，不每个字段都追问
- 目标是减少来回，把"核对"集中在一次拟稿里完成

## 服务目录策略（最易出错，单独说）

服务目录是三级树（`list_service_tree` 返回）。三级职责不同，策略不同：

| 层级 | 字段 | 策略 |
|---|---|---|
| L1（大类） | 不进 body，仅上下文 | agent 判断不了 → 用 Config `default_service_l1` |
| L2（二级 = `service_type`） | body `serviceType` | agent 判断不了 → 用 Config `default_service_l2` |
| **L3（三级 = 问题类型 = `service_sub_type`）** | body `serviceSubType`，且是 `get_replenish_template` 的 `leaf_id` | **必须按内容判断，不用 `default_service_l3`** |

**L3 为什么不能用默认**：L3 既是问题分类、又是补单模板的 `leaf_id`——错了模板也错。
`default_service_l3` 字段是给应用内补单 UI（cascader 初始选中）用的，本 skill 不依赖。

**L3 判断方法**：

1. 先定 L1→L2（判断得到就用，判断不到走默认）
2. `list_service_tree()` 定位到该 L2，取 L2 下挂的 L3 叶子清单（约 6 个，分类明显）
3. 在这个小范围里按内容匹配问题类型：
   - **命中明确** → 直接用
   - **2-3 个候选混淆** → 拟稿列候选让用户选
   - **无命中** → 拟稿标空，附 L2 下 L3 清单让用户选

## 流程：先查后一次性确认

### 1. 解析输入

从自然语言 / 对话提取：项目名、报告人、问题内容。识别明显缺失的关键信息（如完全没提项目或人）。

### 2. 并发反查（一次性发出，不等用户）

- `search_customer_groups(keyword=项目名)` → 客户组 cgId + name
- `search_base_persons(keyword=报告人)` → requestor userId + name
- `list_service_tree()` → 定位 L1>L2（判断或默认），取 L2 下 L3 叶子，按内容匹配 L3

### 3. 取模板

`get_replenish_template(leaf_id=L3 的 stId)` → `create_template_id`

### 4. 拼拟稿

结构化字段清单 + 来源标注 + ⚠️ 标 agent 判断项（模板见下）。

### 5. 用户确认

用户说"确认" → 进第 6 步；"改某项" → 改完重新确认。**未确认不建单。**

### 6. 建单 + 取单号

`create_ticket(...)` → `code==800` 时 `data` 为新单 incidentId（内部 ID，非 IM 单号）
→ `get_detail(id=incidentId)` 取 incidentCode（IM 开头）回报给用户。

## detail 加工规范

整理汇总原文，**不扩展、不结构化、不追问补全**：

- 对话 / 片段归并成连贯描述，按时间或逻辑顺序串联
- 去问候、表情、寒暄等噪音，保留问题相关信息
- HTML 排版：换行 `<br>`、段落 `<p>`、重点 `<strong>`、列表 `<ul>/<li>`
- **不要**套"现象 / 影响 / 已采取措施 / 期望"四段结构（用户明确不要扩展）
- 保持原意，不加报告人没说的内容；信息不足就照实汇总，不追问补全
- 主题 `order_subject`：从内容提炼一句话（如"VPN 登录超时"），不放对话原文

## 拟稿模板

```
📋 补单拟稿（⚠️ = agent 判断项，请重点核对）
服务目录：IT基础设施 > 网络与通信（默认）> VPN连接问题 ⚠️（按"登录超时"匹配，L2 下 6 叶子）
客户组：  得力集团（cgId=…，搜"得力"命中 2 条取首条，另有"得力浙江"）⚠️
提单人：  张三（userId=…，搜"张三"命中 1 条）         ← 报告人
支持组：  默认支持组（应用配置）
主题：    VPN登录超时 ⚠️（提炼自原文）
详情：    <整理汇总后的 HTML 预览> ⚠️
模板：    自动取（createTemplateId=…）

确认建单？或告诉我要改哪项。
```

每项都带**来源**（默认值 / 搜索命中 / 提炼），让用户能快速核对 agent 判断对不对。

## 降级与异常

- **客户组 / 提单人搜不到**：换关键字（如简称→全称、去掉前缀）重试；仍无 → 问用户要准确名称
- **客户组 / 提单人重名**：拟稿列命中项 + 其他同名项，标 ⚠️ 让用户选，不自动拍板
- **服务目录 L1/L2 默认未配置**：拟稿里服务目录标空，问用户选 L1/L2（或让用户先去应用配置默认值）
- **token 中途失效**：报错并提示"请在 itsm-manager 应用重新登录"（MCP 借用应用登录态，应用重登后重试）
- **建单失败（`code != 800`）**：透传后端 `msg`，保留拟稿让用户改完重试，不丢字段

## 边界

- **只做补单**：建单 + `get_detail` 取 incidentCode 回报。建单后的接单 / 回复 / 解决属工单处理，走 `itsm` skill
- **不支持批量**：同一请求识别出多个独立问题时，提示用户拆分后逐张走流程
- **detail 只整理汇总**：不扩展、不结构化、不追问补全报告人没说的内容
- **未确认不建单**：拟稿必须经用户明确确认才调 `create_ticket`

## 工具加载

schema 默认 deferred。流程开始前先加载本 skill 用到的 6 个 tool：

```
ToolSearch query="select:mcp__itsm-tools__list_service_tree,mcp__itsm-tools__get_replenish_template,mcp__itsm-tools__search_customer_groups,mcp__itsm-tools__search_base_persons,mcp__itsm-tools__create_ticket,mcp__itsm-tools__get_detail"
```
