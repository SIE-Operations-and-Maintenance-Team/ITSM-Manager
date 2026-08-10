---
name: itsm-service-report
description: >
  把 ITSM 导出的"服务报告"xlsx 改造成对外交付的"客户运维服务报告"xlsx。用户给出
  ITSM 导出 xlsx 路径、要求生成客户运维服务报告时使用。覆盖：抽取工单、统计服务摘要
  4 指标、按主题把工单分到 5 大类、填 13 列工单表、生成服务总结、重建原生饼图、套用
  模板格式。触发词：客户运维服务报告、服务报告、ITSM 导出、工单报告、运维报告、
  服务总结、把这个 xlsx 改成客户报告。即使用户只贴一个 xlsx 路径说要生成报告，
  也应识别为本 skill。
---

# 客户运维服务报告生成（itsm-service-report）

把 ITSM 导出的"服务报告"xlsx 改造成对外交付的"客户运维服务报告"。格式忠于 skill 内
`template.xlsx`（一份填好的真实报告作格式基底），数据来自原始导出。

## 依赖

- **officecli ≥ 1.0.143**（xlsx 操作 / chart / validate / view）。命令权威见 `officecli` skill，本 skill 不重复列命令；属性拿不准时 `officecli help xlsx <element>`。
- **itsm-tools MCP**（`get_ticket_by_code`，分类信息不足时查工单详情）。schema 默认 deferred，先 `ToolSearch query="select:mcp__itsm-tools__get_ticket_by_code"` 加载。
- **skill 内 `template.xlsx`**：格式基底，复制即继承合并区/列宽/字体/边框/logo。**它内部含旧示例数据（得力客户 48 行工单），复制后必须清空再填新数据。**
- **三个 references（本 skill 的智力资产，必读）**：
  - [`references/template-anatomy.md`](references/template-anatomy.md) — 模板实测结构（行号/合并区/锚点）+ **chart 与图片操作命令**。**所有行号、合并区、命令查这里，不硬编码、不猜测。**
  - [`references/classification-prompt.md`](references/classification-prompt.md) — 工单 5 类分类 prompt + 少样本。
  - [`references/summary-prompt.md`](references/summary-prompt.md) — 服务总结三段 prompt + 数据契约。

## 接口

- **入参**：
  - `<文档路径>`（必填）：ITSM 原始导出 xlsx（约 18 列）。
  - 可选自由文本：服务总结的导向描述（基调/重点/想强调的内容），无则按通用基调生成。
- **输出路径**：默认 `<调用时工作目录>/doc/客户运维服务报告_<项目名>(<日期范围>).xlsx`。
  - `<项目名>` ← 原始 A3；`<日期范围>` ← 原始 A4 文本中的日期段。
  - 项目名含 Windows 禁用字符（`\ / : * ? " < > |`）替换为下划线。
  - 用户明确指定输出路径时覆盖默认。
- **全程 UTF-8 / 中文**（规避 Windows GBK 静默损坏）。

## 工作流

### 0. 前置检查
- `officecli --version` 可用且 ≥ 1.0.143。
- `template.xlsx` 存在（本 skill 目录下）。
- 输入可读；被占用则复制副本到临时目录读取（不强制用户关 Excel）。**输出写入时若被占用，提示用户关闭后重试，不静默失败。**
- **先实测原始文件结构**：`officecli view <输入> outline` + `view <输入> text | head -15`，确认表头行号、工单起始行、列名（不同导出可能有差异）。

### 1. 抽取原始数据（按表头列名匹配，不依赖固定列字母/行号）
- 项目名 = 原始 A3。
- 报告日期 = 原始 A4 文本，提取日期段。
- 工单列表 = 表头行下一行起至最后一行。**按表头列名定位**各列（工单号/主题/状态/客户/客服组/创建人/处理人/创建于/响应于/完成于/服务类型/解决方案；丢弃抄送人/优先级/满意度/解决时长/SLA标准时长）。原始"服务目录"列**不信任**（实测常批量填同一个值，失真）。

### 2. 建输出文件
- 输出目录（默认 `<CWD>/doc/`）不存在则先创建。
- 复制 `template.xlsx` → 输出路径（`cp` 或 `powershell Copy-Item`，UTF-8 路径用单引号）。后续写操作都在输出文件上，**不动 template**。

### 3. 统计服务摘要 4 指标（从工单列表直接算，不依赖原始摘要字段）
- 工单总数 = 工单行数。
- 完成工单 = 状态 ∈ {已解决, 已关闭}。
- 待完成工单 = 状态 ∈ {处理中, 暂挂, 待受理}。状态值不在枚举内 → 计入待完成，并在交付报告列出该异常状态值。
- 服务请求人数 = 创建人去重计数。
- 与原始摘要值交叉校验；不一致以工单列表统计为准并在报告备注。

### 4. 工单分类到 5 大类
- 输入 = 每个工单的「主题 + 解决方案」；分批（每批 ~20 条）套用 [`classification-prompt.md`](references/classification-prompt.md)。
- 对返回 `confidence=low` 的工单，用 `get_ticket_by_code(code=IM单号)` 取描述+历史回复，并入主题+解决方案重跑 prompt。
- itsm-tools 不可用 → 跳过详情查询，按现有信息 best-guess 并报告降级。
- 仍 `low` → 归 `BUG问题` 并把单号列入"存疑清单"。
- 5 类固定枚举（不得新增/改名）：`需求与性能优化、服务器健康检查、服务故障处理、问题咨询、BUG问题`。
- **0 计数类忽略**：统计分类计数后，计数为 0 的类别在第 7 步服务总结、第 8 步饼图中**一律不呈现**——只写/只画计数 > 0 的类（summary-prompt 数据契约同规则）。

### 5. 填工单列表
- 按 [`template-anatomy.md`](references/template-anatomy.md) 的"工单示例行范围"，清空模板旧工单行（row11 起、A 列为 IM 单号的所有行）：逐格 `set --prop value=""` 或 `remove` 行，不留残数据。
- 按输出 13 列顺序写入新工单数据，**保持原始工单顺序**（默认不重排）。
- 行数适配：新工单多于模板行 → `officecli add <输出> /Sheet1 --type row --index <N>`（xlsx row 的 `--index` 1-based）追加，并 `set` 继承工单行样式（template-anatomy 记录的微软雅黑14pt/四边thin边框/wrapText/居中/行高57.6pt）；少于 → 清空全部剩余行。
- "服务目录"列（L）填第 4 步分类结果（**spec 5 大类**，非模板的细分类）。

### 6. 写服务摘要（template-anatomy 的 row6 四个合并区）
- 按四个合并区主单元格地址与文本格式（见 template-anatomy"服务摘要"表），写入 `工单总数：<n>` / `完成工单：<n>` / `待完成工单：<n>` / `服务请求人数：<n>`（第 3 步的值）。

### 7. 生成服务总结 + 服务概览（template-anatomy 的 A7:J8 合并区，富文本）
- 按 [`summary-prompt.md`](references/summary-prompt.md) 的输入数据契约收集字段（4 指标 + 5 类计数 dict + 主要联系人 + 导向描述 + 是否重大故障）；**0 计数类在总结里不呈现**（summary-prompt 第 3 段规则）。
- 主要联系人：创建人按频次 top 1–3，清洗前缀（如 `优特格尔_吴锦涛`→`吴锦涛`、`得力_王梦杰`→`王梦杰`、`SIE_周俊`→`周俊`）。
- 把契约 JSON + prompt 喂给 LLM，输出**带 `**加粗**` 标记的三段文本**（服务总结 + 服务概览，summary-prompt 已规定哪些标题/标签加粗、哪些正文常规）。
- **写入富文本（字体粗细 + 字体锁定跟模板一致）**：把 LLM 输出按 `**...**` 边界切分为 runs，**每个 run（无论 bold）都注入字体** `font:"微软雅黑", size:"16pt", color:"000000"`（= 模板 A7 字体），写 `A7`（合并区 `A7:J8` 主单元格，**A–J 列**；K7:M8 留给饼图）：
  ```bash
  officecli set <输出> '/Sheet1/A7' --prop type=richtext --prop runs='<解析出的 runs JSON>'
  # runs 形如（每 run 都带字体）：
  # [{"text":"服务总结：\n","bold":true,"font":"微软雅黑","size":"16pt","color":"000000"},
  #  {"text":"衷心感谢…","bold":false,"font":"微软雅黑","size":"16pt","color":"000000"},…]
  ```
- 解析规则：`**X**` → `{"text":"X","bold":true,"font":...,"size":...,"color":...}`；标记外文本 → 同结构但 `bold:false`；把 `**` 剥离、不要写进 cell。**务必每 run 带 font**——officecli 写 richtext 会重建 styles，runs 不带 font 会继承到错误字体（宋体 11pt 红色），详见 template-anatomy"服务总结富文本"。bash 传 JSON 用单引号包裹，换行写成 `\n`。
- 校验：`officecli get <输出> '/Sheet1/A7' --json` 应见 `format.richtext=true`、`children` 含多个 `run[N]`；字体核对以解包 `xl/sharedStrings.xml` 对应 `<si>` 里 `<rPr>` 含 `<rFont val="微软雅黑"/><sz val="16"/>` 为准。
- ⚠️ **richtext 可能被后续命令加 `<is>` 覆盖**：officecli 生成流程中 A7 可能出现 `<v>N</v>` + 纯文本 `<is>` 并存，WPS 渲染 `<is>` 丢 bold 区分（实测用户报告全加粗根因）。第 8 步 editAs 后处理的 python 脚本会一并删掉多余 `<is>`（template-anatomy 步骤 2.5），无需单独操作；**最终以 WPS 打开核对**"标题加粗、正文常规"为准（officecli screenshot 不暴露此问题）。
- 生成失败 → 回退"数据驱动最小版"（见 summary-prompt 降级章节，仍保留 `**...**` 标记 → runs）并报告。

### 7.5 行高适配（A7 内容驱动 + 让 K7:M8 方正给饼图）
- **模板 row7/row8 合计 717pt 只是占位，生成时必须按本次 A7 实际内容重算**——否则文字区大片留白、K7:M8 饼图区过高导致饼图上下留白。计算方法见 [`template-anatomy.md`](references/template-anatomy.md) "row7/row8 行高适配"章节：PIL 测 A7 全文在 A7:J8 宽度下的折行数 → `H = 折行数 × 16pt × 1.2`；读 K–M 列宽对应宽度 `W`（≈477pt）；设 `row7+row8 = max(H + 10–15% 余量, 接近 W)`，让 K7:M8 近正方形（饼图直径 = min(W, row7+8) 最大化、留白最小）。
- 写入：`officecli set <输出> /Sheet1/row[7] --prop height=<h7>` 与 `row[8]`（或一条 batch）；分配 ~52/48（row7 略大，两行同属合并区，分配不影响 A7:J8 / K7:M8 显示）。
- 校验 `officecli view <输出> issues` = 0（A7 无 text-overflow）；仍报 overflow 则按 issues 建议高度加高 row7/8 重验。
- ⚠️ **客户手动编辑过输出文件后**（如在 WPS 里调 A7 文字/换行），row7/row8 与 chart 的 `editAs` 都可能被 WPS 重写——交付前需重算 H、并重跑第 8 步 editAs 后处理。

### 8. 重建饼图（K7:M8，嵌入单元格）
- 严格按 [`template-anatomy.md`](references/template-anatomy.md) "chart 操作命令"章节：
  1. **清掉 K7 的 WPS DISPIMG 公式**（模板饼图是 K7 单元格内嵌图片公式，留着会显示旧饼图）：`officecli set <输出> '/Sheet1/K7' --prop value=""`。
     - 模板当前**无浮动 picture 饼图**（旧版曾为 `picture[2]`，现已被 DISPIMG 取代），故无需 `remove picture[2]`；若 `query picture` 出现饼图 picture 再 remove。
  2. 用章节中确认的 `add --type chart` 命令创建原生 pie chart（内联 `data=` / `categories=` / `colors=`——**只含计数 > 0 的类**，按固定枚举顺序；`dataLabels=percent` + `labelPos=bestFit` + `legend=right` + `varyColors=true` + `title=none` + **`anchor='K7:M8'`**）。
- **只画计数 > 0 的类**：按固定枚举顺序（需求与性能优化 / 服务器健康检查 / 服务故障处理 / 问题咨询 / BUG问题）遍历 `categoryCounts`，**跳过计数为 0 的类**，拼出 `data`/`categories`/`colors` 三个等长列表。**`colors` 按类名固定映射**（不按位置），保证每类颜色跨报告稳定：需求与性能优化=4472C4、服务器健康检查=ED7D31、服务故障处理=A5A5A5、问题咨询=FFC000、BUG问题=5B9BD5（见 template-anatomy 配色表）。
  - 例：若仅"服务故障处理 35、问题咨询 30、BUG问题 11"有计数，则 `data='工单数:35,30,11'`、`categories='服务故障处理,问题咨询,BUG问题'`、`colors='A5A5A5,FFC000,5B9BD5'`。
- ⚠️ `anchor='K7:M8'` → drawing XML 生成 **twoCellAnchor**（K–M 列、row7–8）。**不要 `set chart --prop width/height`**——实测会把 anchor 终点改写到 T53，chart 跑出 K–M 列。`officecli get chart` 显示 `width=96 height=15` 是换算显示 bug，实际锚点为 K7:M8（Excel 里铺满约 460×717pt），勿误判。详见 template-anatomy"chart 操作命令"。
- **3. 显式 editAs=twoCell 后处理（确保随单元格大小互动，必做）**：
  - **先释放 resident 句柄**：`officecli close <输出>`。officecli 是常驻进程（resident），命令返回后仍持有文件**写锁**，不 close 则后续 python `os.replace` 覆盖会 `PermissionError`（实测反复踩到）。`close` 把改动 flush 落盘并释放句柄。
  - **再跑 python 后处理（一脚本两件事）**：① 改 `xl/drawings/drawing1.xml` 给 chart 的 twoCellAnchor 加 `editAs="twoCell"`（logo 的 oneCell 不动）；② 删 richtext cell（A7 服务总结等）里多余的纯文本 `<is>`——officecli 生成流程可能给 richtext cell 塞入纯文本 `<is>` 覆盖富文本 `<v>`，**WPS 渲染 `<is>` 会丢 run 级 bold 区分 → 服务总结/概览"全加粗、分不清主次"**（officecli `view screenshot` 走 `<v>` 富文本看不出此问题，**必须 WPS 实地核对**）。脚本见 template-anatomy"显式 editAs=twoCell 后处理"步骤 2.5。close 后 `os.replace` 一次成功（脚本带重试兜底 Excel/WPS 占用）。
  - 后处理后 chart 随 K–M 列宽 / row7-8 行高变化移动并缩放（moveAndSizeWithCells）= 嵌入单元格 + 与单元格大小互动。
  - **此步为生成流程最后一步**：之后不再用 officecli 改该文件（否则 resident 重新持锁、且可能覆盖 editAs）。第 10 步 validate 会重新 open resident 只读最终文件，不碍事。

### 9. 写标题区
- A3 = 项目名（原始 A3）：`officecli set <输出> /Sheet1/A3 --prop value=<项目名>`。
- A4 整段重写（**officecli 对 xlsx 不支持 --find/--replace**，实测 `matched 0 occurrences` + `UNSUPPORTED props: find`）：`officecli set <输出> '/Sheet1/A4' --prop value=$'客户服务报告\n报告日期：<原始日期段>'`（bash `$'...'` 使 `\n` 为真实换行；文字保持模板原样，仅日期段换成原始的）。日期段格式见 template-anatomy。

### 10. 验证与交付
- `officecli validate <输出>`：验证标准 = **无新增 schema 错误**（模板既有的 2 个 WPS `etCustomData` 扩展错误属正常，见 template-anatomy"已知既有问题"，不计入失败）。
- `officecli view <输出> issues`：无结构性问题。
- `officecli view <输出> screenshot -o <同名.png>` 核对标题/4指标/服务总结/表头/工单行。**注意：screenshot 不渲染原生 chart**（见 template-anatomy"screenshot 局限"），饼图核对用 `officecli query <输出> chart`（确认 `/Sheet1/chart[1]` chartType=pie 存在）+ **在 Excel/WPS 打开输出文件**确认扇区/标签/图例/配色视觉。
- 向用户报告：输出路径、4 指标、5 类分类计数及占比（注明四舍五入）、主要联系人、任何兜底/降级/存疑工单单号清单。

## 列映射（原始 → 输出 13 列）

> 按**表头列名**匹配原始列，列字母仅对照（原始约 18 列 A–R）。

| 输出列 | 来源 |
|---|---|
| 工单号 | 原始 工单号(A) |
| 主题 | 原始 主题(B) |
| 状态 | 原始 状态(C) |
| 客户 | 原始 客户(D) |
| 客服组 | 原始 客服组(E) |
| 创建人 | 原始 创建人(F) |
| 处理人 | 原始 处理人(G) |
| 创建于 | 原始 创建于(I) |
| 响应于 | 原始 响应于(J) |
| 完成于 | 原始 完成于(K) |
| 服务类型 | 原始 服务类型(N) |
| 服务目录 | 第 4 步分类结果（5 大类，非原始 O 列） |
| 解决方案 | 原始 解决方案(P) |

删除（不写入）：抄送人(H)、优先级(L)、满意度(M)、解决时长(分钟)(Q)、SLA标准时长(分钟)(R)。

## 模板继承（复制即得，不手动重建）

合并区、列宽、行高、字体、字号、颜色、表头底色、边框、logo 图片 —— 均随复制保留。
只改动：A3 文本、A4 日期段、row6 四指标、工单数据（清旧填新）、**A7:J8 服务总结+服务概览（富文本，区分字体粗细）**、**K7:M8 饼图（清 DISPIMG → 原生 chart，twoCellAnchor 嵌入）**。

> 布局要点（2026-08-07 重排后）：row7–row8 左右分栏——A7:J8（A–J 列）放服务总结+服务概览文字，K7:M8（K–M 列）放饼图，两区共用 717pt 高的矩形并排。

## 错误处理与边界

- 输入被占用 → 复制副本读取。
- 输出被占用 → 提示用户关闭后重试（不静默失败）。
- itsm-tools 不可用 → 仅用主题+解决方案分类，跳过详情查询，报告降级。
- 分类置信度低且查 ITSM 后仍不确定 → 归 `BUG问题` + 标注存疑单号清单。
- 工单列表为空 → 中止并提示。
- 服务总结生成失败 → 回退数据驱动最小版并报告。
- 全程中文 / 文件 I/O 用 UTF-8。
