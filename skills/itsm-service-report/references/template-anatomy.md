# template.xlsx 实测结构（单一事实源）

> 行号 / 合并区 / 锚点 / 命令以本文件为准。模板替换 `skills/itsm-service-report/template.xlsx` 后，**重跑下方 officecli 探测命令更新本文件**。
> 探测日期：2026-08-07（当次会话已对模板重排后的版本重新实测）。officecli 版本：1.0.143。
> 模板性质：**非空模板**——装着"得力客户 2025 年度真实报告"作为格式基准与生成基底。生成新报告 = 复制后清旧数据、填新数据。

## 已知既有问题（非本次引入，不阻塞）

`officecli validate` 报 schema 错误，均为 WPS 自定义扩展（`wps.cn/officeDocument/2017/etCustomData`，挂在 autoFilter 上），属 WPS 生成 xlsx 的常见非标准扩展，**不影响数据读写 / chart / 截图**。SKILL.md 第 10 步验证标准 = "无**新增** schema 错误（这些 WPS 既有错误除外）"。

## sheet
- 名称：`Sheet1`（唯一 sheet，13 列 A–M）

## 列宽（字符单位，实测）
A24.89 / B34.22 / C17 / D14.22 / E14.78 / F17.04 / G15.78 / H23.78 / I24.22 / J23.67 / **K32.67 / L23.67 / M32.41**。
- K–M 三列合计约 88.75 字符（≈460pt 宽），是饼图区的横向空间。

## 标题区
- `A3`（项目名占位）：文本 `XXXXX`，合并区 `A3:M3`，微软雅黑 36pt 加粗，水平+垂直居中。
- `A4`（标题）：文本 `客户服务报告\n报告日期：2025.01.01-2025.12.31`（`\n` 为换行），合并区 `A4:M4`，微软雅黑 20pt 加粗，wrapText，水平+垂直居中。
  - **日期段格式**：`2025.01.01-2025.12.31`（点分隔，连字符连接起止）。生成时 **A4 整段重写**（officecli 对 xlsx 不支持 --find/--replace，实测报错）：`officecli set <file> '/Sheet1/A4' --prop value=$'客户服务报告\n报告日期：<原始日期段>'`。
- `A5`：`服务摘要：`（标签，不动）。

## 服务摘要（row6 四指标合并区，不动）

| 合并区 | 文本格式（写入主单元格，即合并区左上） |
|---|---|
| `A6:B6` | `工单总数：<n>` |
| `C6:F6` | `完成工单：<n>` |
| `G6:I6` | `待完成工单：<n>` |
| `J6:M6` | `服务请求人数：<n>` |

字体：微软雅黑 16pt **加粗**，水平 left + 垂直 center。模板旧值：49 / 48 / 1 / 6（生成时覆盖为新客户统计）。

## 服务总结 + 服务概览区（A7:J8，文字；K7:M8，饼图）

**row7–row8 是左右分栏布局**（2026-08-07 重排，区别于旧版 A7:M8 全宽）：

| 合并区 | 用途 | 主单元格格式 |
|---|---|---|
| `A7:J8` | **服务总结 + 服务概览文字**（A–J 列，10 格） | 微软雅黑 16pt，水平 left + 垂直 top，wrapText |
| `K7:M8` | **饼图区**（K–M 列，3 格） | 见下"饼图区" |

- 行高：`row7 = 386pt`、`row8 = 331pt`，合计 **717pt**——A7:J8 文字区与 K7:M8 饼图区共用这块高大矩形，左右并排。**（模板占位值；生成时按 A7 实际内容重算，见下「row7/row8 行高适配」章节）**
- 模板 A7 含完整三段范例（开篇感谢+主要联系人 → 服务概览零P0/主动巡检/完成率 → 工单类型分布按占比每类"服务聚焦+价值体现"）。**这是 summary-prompt.md 的风格范本。** 生成时整段覆盖为新客户的总结文本。
- **字体粗细**：模板 A7 的标题/小标题/类别名加粗、正文常规——靠**富文本 runs** 实现（单元格级 `font.bold=true` 是默认值，正文 run 覆盖为非加粗）。生成时必须按"服务总结富文本"章节写 `type=richtext` + `runs`，不能只写纯文本（否则全加粗或全常规，丢失层级）。详见下方"服务总结富文本（字体粗细）"章节。

## row7/row8 行高适配（生成时按 A7 内容重算 + 让 K7:M8 方正）

模板 row7+row8=717pt 只是占位。不同客户 A7 文字长度不同，**必须重算**，否则文字区大片留白、K7:M8 过高致饼图上下留白。

**计算（python + PIL，微软雅黑 `C:/Windows/Fonts/msyh.ttc`）**：
1. 读 A7 富文本全文（openpyxl `ws['A7'].value`），按 `\n` 分逻辑行。
2. A7:J8 区域像素宽 = `sum(width_i*7 + 5 for A–J)`（列宽合计 209.6 → ≈1517px ≈1138pt）。
3. `ImageFont.truetype(msyh, 21)`（16pt≈21px），每逻辑行贪婪逐字折行（`font.getlength(cur+ch) ≤ 区域宽`），累加物理行数 N。
4. 所需高度 `H = N × 16 × 1.2`（pt；与 officecli issues 的"N lines at Szpt need N×Sz×1.2"同算法）。
5. K–M 列宽对应宽 `W = sum(width_i*7+5 for K–M)/96*72`（≈477pt）。
6. 设 `row7+row8 = max(H × 1.10~1.15, W)`（文字余量 + 逼近 W 让 K7:M8 近正方形，饼图直径=min(W,row7+8) 最大化、留白最小）；分配 row7:row8 ≈ 52:48（row7 略大，两行同属合并区，分配不影响显示）。

实测（2026-08-08，优特格尔 104 单）：客户手动补换行后 A7 = 20 逻辑行 / 872 字符 → 折 22 物理行 → H=422pt → 设 row7+row8=490pt（255+235），K7:M8=477×490 近方正，饼图直径≈477pt，`view issues`=0。

写入：`officecli set <输出> /Sheet1/row[7] --prop height=<h7>` + `row[8]`（或一条 batch）。校验 `officecli view <输出> issues`=0；A7 报 text-overflow 则按 issues 建议高度加高重验。

## 饼图区 K7:M8

- 模板 K7:M8 主单元格 K7 存的是 **WPS 单元格内嵌图片公式 `=DISPIMG("ID_...",1)`**（饼图 PNG 预渲染图，随单元格内嵌）。**不是** officecli 可见的 picture（`query picture` 只返回 logo）。
- 生成时**必须清掉 K7 的 DISPIMG 并重建为原生 chart**：DISPIMG 是 WPS 私有、officecli 无法更新其图片数据，留着会显示旧饼图。
- 清空：`officecli set <输出> '/Sheet1/K7' --prop value=""`（实测可行，Warning 提示替换公式即预期效果）。

## 表头与工单行
- `A9`：`服务明细`（分区标签，合并区 A9:M9，不动）。
- 表头行：`row10`，13 列名（顺序）：
  `工单号、主题、状态、客户、客服组、创建人、处理人、创建于、响应于、完成于、服务类型、服务目录、解决方案`，微软雅黑 14pt 加粗，居中。
- 工单示例行范围：`row11` 起至模板末行（实测约 47–48 行示例，末行可能浮动）。
  - **清空策略**：清空 `row11` 起、A 列为 `IM` 单号的所有行（覆盖可能的行号偏差）。新工单从 row11 写。
  - 行数适配：新工单 > 示例行数 → `officecli add <file> /Sheet1 --type row --index <N>`（xlsx row 的 `--index` 为 1-based）追加，并 `set` 继承下方样式（见"工单行样式"）；< 示例行数 → 清空全部剩余示例行（A 列置空），不留残数据。
- **工单行样式**（实测 row11）：字体 `微软雅黑`、字号 `14pt`、四边 `thin` 边框、`wrapText=true`、水平 `center` + 垂直 `center`、行高 `57.6pt`。追加新行时用 `set --prop` 复制这些属性。
- **行高溢出提示**：固定行高 57.6pt + wrapText 下，超长主题（5 行+）会触发 `officecli view issues` 的 `text-overflow` 提示（实测优特格尔 104 单约 3 条），数据完整、Excel 可读。officecli 的 `customHeight` 是 get-only（无法直接 set 清除做 auto-fit）；如需无溢出，在 Excel 中选中工单行双击边界 auto-fit，或对个别超长行 `set /Sheet1/row[N] --prop height=84`。

## 服务目录列（L 列）重要说明

- 模板 L 列示例用的是**细分类**（约 8 类：`需求与性能优化、功能问题处理、服务故障处理、服务器漏洞修复、服务器健康检查、业务问题咨询、接口问题处理、部署自动化`）。
- **生成时按 spec 固定 5 大类覆盖**（`需求与性能优化、服务器健康检查、服务故障处理、问题咨询、BUG问题`），分类由 SKILL.md 第 4 步 + `classification-prompt.md` 基于"主题+解决方案"判定，**不直接搬运模板 L 列细分类**。

## 图片

| 用途 | officecli 路径 | 锚点（drawing XML twoCellAnchor） | 类型 | 处理 |
|---|---|---|---|---|
| logo | `/Sheet1/picture[1]` | `A1:B3`（col0 row0 → col1 row2） | picture (PNG) | **保留** |
| 饼图 | （模板里是 K7 的 DISPIMG 公式，非 picture） | K7:M8 | WPS DISPIMG 公式 | **清空 K7 + 重建为原生 chart**（见下） |

`officecli query <file> picture` 当前只返回 logo；饼图不在此列。

## 配色（饼图 5 色）

按**类名固定映射**（不按位置）——无论哪些类存在、顺序如何，每类颜色恒定，跨报告一致：

| 类别 | 色值 |
|---|---|
| 需求与性能优化 | `#4472C4`（蓝） |
| 服务器健康检查 | `#ED7D31`（橙） |
| 服务故障处理 | `#A5A5A5`（灰） |
| 问题咨询 | `#FFC000`（黄） |
| BUG问题 | `#5B9BD5`（浅蓝） |

构造饼图命令时，按固定枚举顺序遍历 `categoryCounts`，**跳过计数为 0 的类**，把剩余类的色值按序列入 `colors=`（与 `categories=`、`data=` 三者等长、同序）。饼图单系列 + `varyColors=true` 按数据点变色；逐点配色以 Excel 渲染为准，若 `colors` 不逐点生效，改用 per-point（`series1.point{N}.color`）或 `preset=corporate`（待办，见 handoff）。

## 服务总结富文本（字体粗细 + 字体锁定，A7:J8）

officecli 支持单元格富文本，属性：`type=richtext` + `runs`（JSON 数组）。**每个 run 必须显式带字体字段**，不能只写 bold 靠继承：

```json
{"text":"...","bold":true,"font":"微软雅黑","size":"16pt","color":"000000"}
```

- ⚠️ **为何 runs 必须带 font（不能继承单元格样式）**：模板 A7 单元格样式 `s=9`，原始 `cellXfs[9]` 不直接绑 font（靠行/列样式继承到微软雅黑 16pt dk1）。但 **officecli 写 `type=richtext` 会重建 `styles.xml`**，实测写后 `cellXfs[9]` 被关联到 `font[11]=宋体 11pt 红色(FFFF0000)`——若 runs 不带 font，正文会继承到宋体 11pt 红色，字体完全错。所以**每个 run 显式锁字体**。
- 字体取值（与模板 A7 一致）：`font="微软雅黑"`、`size="16pt"`、`color="000000"`。实测 officecli 把 `color="000000"` 转为 `<x:color theme="1"/>`（= dk1 主题黑本体，与模板一致）并自动补 `<x:charset val="134"/>`——run 的 rPr 最终为 `<b/><sz val="16"/><color theme="1"/><rFont val="微软雅黑"/><charset val="134"/>`（加粗 run 多一个 `<b/>`），与模板 A7 字体定义吻合。
- 加粗 `bold:true`：各级标题、小标题、类别名行、`服务聚焦：`/`价值体现：` 子标签、成果要点标签。
- 加粗 `bold:false`：所有描述性正文。
- 换行：在 run 的 `text` 内用 `\n`（bash 用 `$'...'` 让 `\n` 为真实换行；runs JSON 里直接写 `\n`）。
- 写入命令骨架（runs 内容由 summary-prompt.md 输出 `**...**` 标记 + 执行期解析并给每 run 注入字体生成）：
  ```bash
  officecli set <输出> '/Sheet1/A7' --prop type=richtext \
    --prop runs='[{"text":"服务总结：\n","bold":true,"font":"微软雅黑","size":"16pt","color":"000000"},{"text":"衷心感谢…信任与支持。","bold":false,"font":"微软雅黑","size":"16pt","color":"000000"},…]'
  ```
- 读回校验：`get '/Sheet1/A7' --json` 应见 `format.richtext=true`、`children` 含多个 `run[N]`；解包看 `xl/sharedStrings.xml` 对应 `<si>` 里每个 `<rPr>` 都含 `<rFont val="微软雅黑"/><sz val="16"/><color rgb="FF000000"/>` + 加粗 run 多一个 `<b/>`。

## chart 操作命令（实测确认）

### 清 K7 的 DISPIMG（重建前必做）
```bash
officecli set <输出> '/Sheet1/K7' --prop value=""
```
Warning 提示 `replacing with literal value` 即预期（清掉 DISPIMG 公式）。模板当前**无浮动 picture 饼图**（旧版曾为 picture[2]，现已被 DISPIMG 取代），故无需 `remove picture[2]`；若未来模板恢复浮动饼图图片，再 `officecli remove <输出> '/Sheet1/picture[2]'`。

### 创建原生饼图（只含计数 > 0 的类，内联数据，嵌入 K7:M8）
用内联 `data=` + `categories=` + `colors=`（不污染临时单元格）。**按固定枚举顺序遍历 `categoryCounts`，跳过计数为 0 的类**，把剩余类按序填入三个等长列表（计数为 0 的类不出现在饼图里）：
```bash
officecli add <输出> /Sheet1 --type chart \
  --prop chartType=pie \
  --prop data='工单数:<仅 >0 类的计数, 逗号分隔>' \
  --prop categories='<仅 >0 类的类名, 按固定枚举顺序, 逗号分隔>' \
  --prop dataLabels=percent --prop labelPos=bestFit \
  --prop legend=right --prop varyColors=true \
  --prop colors='<仅 >0 类的色值, 按类名映射, 见上方"配色">' \
  --prop title=none --prop anchor='K7:M8'
```
- 构造示例：`categoryCounts = {需求与性能优化:0, 服务器健康检查:8, 服务故障处理:35, 问题咨询:0, BUG问题:11}` → 跳过 2 个 0 计数类，得 `data='工单数:8,35,11'`、`categories='服务器健康检查,服务故障处理,BUG问题'`、`colors='ED7D31,A5A5A5,5B9BD5'`。
- `colors` 按类名固定映射（见上方"配色"表），不按位置——保证每类颜色跨报告稳定。
- `anchor='K7:M8'` → drawing XML 生成 **twoCellAnchor**（from col10 row6 → to col12 row7），饼图落在 K–M 列、与左侧 A7:J8 文字同高（717pt）。officecli 生成的 chart twoCellAnchor **不带 editAs**（OOXML 默认 twoCell = moveAndSizeWithCells），但为 100% 确保所有渲染器都"随单元格移动+调整大小"，**必须后处理显式加 `editAs="twoCell"`**（officecli `set editAs` 报 UNSUPPORTED，只能 raw XML，见下小节）。
- ⚠️ **不要 `set chart --prop width/height`**：实测 `set width=460pt height=700pt` 会把 anchor 终点改写成 `K7:T53`（chart 跑出 K–M 列、越过 row8），破坏布局。尺寸由 K7:M8 twoCellAnchor 自动决定。
- ⚠️ **`officecli get chart` 显示 `width=96pt height=15pt` 是换算显示 bug**——实际 drawing XML 锚点为 K7:M8（实测 `from col10 row6 to col12 row7`），Excel 打开后 chart 铺满 K7:M8 矩形（≈460×717pt）。勿据 96/15 误判。
- 创建后 `officecli query <输出> chart` 应见 `/Sheet1/chart[1]`（chartType=pie）。

### 显式 editAs=twoCell 后处理（确保随单元格大小互动，必做）

officecli 不支持 `set chart --prop editAs`（实测 `UNSUPPORTED props: editAs`）。chart 的 twoCellAnchor 默认无 editAs，靠 OOXML 默认 twoCell 语义。为杜绝个别渲染器按其他默认处理，**生成 chart 后用 python 改 drawing XML 显式加 `editAs="twoCell"`**（只改 chart 那条，logo 的 `oneCell` 不动）。

**前置（必做）：先 `officecli close <输出>` 释放 resident 句柄**。officecli 是常驻进程（resident），命令返回后仍持有文件**写锁**，不 close 则下面 python `os.replace` 覆盖会 `PermissionError`（实测反复踩到；sleep/重试都救不了，必须显式 close）。close 后改动已 flush 落盘、句柄释放，python 即可覆盖。

```bash
officecli close <输出>
# 输出 "Resident closed for <文件>" 即释放成功
```

```python
import zipfile, os, re, tempfile
src = r'<输出文件绝对路径>'  # Windows 路径，正斜杠
# 1) 把原 xlsx 全部条目读进内存（避免反复打开占用句柄）
with zipfile.ZipFile(src) as z:
    items = {it.filename: z.read(it.filename) for it in z.infolist()}
dn = [n for n in items if 'drawing' in n and n.endswith('.xml')][0]
xml = items[dn].decode('utf-8')
# 2) 给"无 editAs 的 twoCellAnchor"加 editAs=twoCell（已有 editAs=oneCell 的 logo 跳过）
new = re.sub(r'<xdr:twoCellAnchor(?!\s+editAs)([^>]*)>',
             r'<xdr:twoCellAnchor editAs="twoCell"\1>', xml)
items[dn] = new.encode('utf-8')
# 2.5) 修复富文本 <is> 覆盖：officecli 生成流程可能给 richtext cell（如 A7 服务总结）
#      额外加一个纯文本 <is>...</is>，与 <v>N</v> 并存。<is> 优先级高于 <v>，
#      WPS 渲染 <is> 纯文本 → 丢失 run 级 bold 区分（标题/正文都一样，看起来"全加粗"）。
#      删掉 t="s" cell 里多余的 <is>（保留 <v>），让富文本 si[N] 重新生效。
sn = [n for n in items if re.search(r'worksheets/sheet\d+\.xml$', n)]
for n in sn:
    sxml = items[n].decode('utf-8')
    snew = re.sub(
        r'(<(?:x:)?c [^>]*t="s"[^>]*>\s*<(?:x:)?v>\d+</(?:x:)?v>)\s*<(?:x:)?is>.*?</(?:x:)?is>',
        r'\1', sxml, flags=re.S)
    items[n] = snew.encode('utf-8')
# 3) 写临时文件再 os.replace 原子覆盖（officecli 刚返回，句柄可能延迟释放 → 带重试）
#    ⚠️ tmp 必须与 src 同盘：os.replace 跨盘报 WinError 17（非 PermissionError，不进重试直接失败）
import time
src_dir = os.path.dirname(src)
for attempt in range(6):  # 最多 6 次 × 0.5s ≈ 3s，覆盖瞬时文件锁
    try:
        fd, tmp = tempfile.mkstemp(suffix='.xlsx', dir=src_dir); os.close(fd)
        with zipfile.ZipFile(tmp, 'w', zipfile.ZIP_DEFLATED) as zout:
            for name, data in items.items():
                zout.writestr(name, data)
        os.replace(tmp, src)
        break
    except PermissionError:
        try: os.remove(tmp)
        except OSError: pass
        if attempt < 5:
            time.sleep(0.5)
        else:
            raise RuntimeError('editAs 后处理 os.replace 多次失败：输出文件被持续占用（Excel/WPS 打开或 officecli 句柄未释放），请关闭文件后重跑本步')
```

- **步骤 2.5 同时修复"服务总结/概览全加粗"bug**：officecli 生成流程可能给 richtext cell（A7 服务总结）额外塞一个纯文本 `<is>...</is>`，与 `<v>N</v>` 并存。`<is>` 优先级高于 `<v>`，**WPS 渲染 `<is>` 纯文本 → 丢失 run 级 bold 区分**（标题和正文都一个样，用户看到"全加粗、分不清主次"；officecli `view screenshot` 走 `<v>` 富文本故看不出问题，**必须 WPS 实地核对**）。步骤 2.5 删掉多余 `<is>`，让 si[N] 富文本重新生效。校验：解包 `xl/worksheets/sheet1.xml`，确认 A7 等 richtext cell 只剩 `<v>N</v>`、无 `<is>`。
- 实测：后处理后 drawing 含 `<xdr:twoCellAnchor editAs="oneCell">`（logo）+ `<xdr:twoCellAnchor editAs="twoCell">`（chart）；A7 的 `<is>` 被清除（富文本 `<v>` 生效）；`officecli validate` 无新增错误；`officecli get chart` 仍可读。
- 根本解法是**前置 `officecli close`**（释放 resident 句柄，见上）——close 后 `os.replace` 一次成功（实测）。脚本里的 `os.replace` 重试只是兜底（万一用户开着 Excel/WPS）。用 `os.replace` 而非 `shutil.move`：原子覆盖。**`tempfile.mkstemp` 必须传 `dir=os.path.dirname(src)`（与 src 同盘）**——默认系统临时目录（常在 C 盘）与输出文件（常在 D 盘等其他盘）跨盘时，`os.replace` 报 `WinError 17`（系统无法将文件移到不同的磁盘驱动器），且该异常**不属于 `PermissionError`**，不进重试、直接抛出（2026-08-08 实测踩到，优特格尔项目输出在 D 盘）。
- 这是生成流程的**最后一步**（之后不再用 officecli 改该文件，避免覆盖 editAs）。
- ⚠️ **客户手动编辑输出文件（WPS/Excel 打开保存）会让 editAs 丢失**：WPS 重写 `xl/drawings/drawing1.xml` 时会去掉 chart 的 `editAs="twoCell"`（2026-08-08 实测：客户在 WPS 调 A7 换行后，chart 的 editAs 从 twoCell 变 none）。**交付前若文件被手动编辑过，必须重跑本后处理补 editAs**；row7/row8 也可能被改，按上方「row7/row8 行高适配」章节重算 H 后一并重设。
- `editAs="twoCell"` = move and size with cells：插入/删除/调整 K–M 列宽或 row7/row8 行高时，饼图随之移动并缩放，即"嵌入单元格 + 与单元格大小互动"。

### 最终核对 anchor（editAs 后处理后）
解包 xlsx 读 `xl/drawings/drawing1.xml`，确认 chart 那条 `<xdr:twoCellAnchor editAs="twoCell">` 且 `<xdr:from>` col=10 row=6、`<xdr:to>` col=12 row=7（= K7:M8）。

### ⚠️ screenshot 局限（重要）
`officecli view screenshot` 的 HTML 快照**不渲染原生 chart**（仅渲染单元格 + 图片）。核对方式：
- chart 存在性与属性：`officecli query <file> chart` + `officecli get <file> '/Sheet1/chart[1]' --json`（确认 chartType=pie / dataLabels=percent / legend=right / varyColors / seriesCount=1）。
- chart 视觉效果（扇区/标签/图例/配色）：**在 Excel/WPS 打开输出文件**核对。
