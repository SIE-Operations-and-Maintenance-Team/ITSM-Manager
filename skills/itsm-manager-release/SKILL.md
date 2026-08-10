---
name: itsm-manager-release
description: >
  ITSM Manager（Tauri 2 桌面应用）发版流程。发布新版本、升级版本号、打 tag、
  触发 GitHub Release、Tauri updater 签名时使用。覆盖：三处版本号同步、annotated tag、
  push 触发 release.yml CI、release notes 补充、签名私钥管理。触发词：发版、release、
  发布版本、打 tag、升级版本、ITSM Manager 发版、itsm-manager release、updater 签名、
  latest.json。仅适用于 ITSM Manager 仓库（SIE-Operations-and-Maintenance-Team/ITSM-Manager）。
---

# ITSM Manager 发版流程

仅适用于 ITSM Manager 仓库（Tauri 2 Windows 应用，origin = `SIE-Operations-and-Maintenance-Team/ITSM-Manager`）。
机制：**push `v<x.y.z>` tag → `.github/workflows/release.yml` 自动构建 + 签名 + 创建 GitHub Release**。

## 铁律（违反必出事）

1. **三处版本号必须同步**，CI 强制校验 `tag(去v) == 三处`，不一致直接失败：
   - `src-tauri/Cargo.toml` 的 `[package].version`
   - `package.json` 的 `version`
   - `src-tauri/tauri.conf.json` 的 `version`
2. **不要手动 `gh release create`**：会与 release.yml workflow 撞，出现「只剩 source code 无 assets」中间态。正确做法是 push tag 等 CI。
3. **签名 env `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 即使空密码也必须设**（= `""`），否则非交互环境 prompt 卡死。CI 已设；本地手动 `cargo tauri build` 须自己 export。

## 发版步骤

设当前版本 `0.1.7`，发布 `0.1.8`：

1. **同步三处版本号** `0.1.7 → 0.1.8`（三文件都改）。
2. **刷 Cargo.lock**：在 `src-tauri/` 下 `cargo check`（本机 cargo 全路径 `C:/Users/lzm04/.cargo/bin/cargo.exe`，Git Bash PATH 常未含）。
3. **commit**（commit body 会成为 Release notes——release.yml 用 `git log -1 --pretty=%B` 取最近 commit body 作 `releaseBody`）：
   ```
   git add src-tauri/Cargo.toml package.json src-tauri/tauri.conf.json src-tauri/Cargo.lock
   git commit -m "chore(release): 版本 0.1.7 → 0.1.8" -m "<release notes，多行分组写在此>"
   ```
4. **创建 annotated tag**：
   ```
   git tag -a v0.1.8 -m "v0.1.8"
   ```
5. **push 触发 CI**：
   ```
   git push origin main
   git push origin v0.1.8
   ```
6. **等 CI**（release.yml，`windows-latest`，首次无缓存约 15 min）：校验版本 → `tauri-apps/tauri-action` 构建（release+lto）→ 用 `TAURI_SIGNING_PRIVATE_KEY` 签名 → 创建 Release（body=commit body）→ 上传 NSIS `ITSM.Manager_0.1.8_x64-setup.exe` + `.sig` + `latest.json`。
7. **补/美化 Release notes**（可选，当 commit body 不够丰富时）：
   ```
   "/c/Program Files/GitHub CLI/gh.exe" release edit v0.1.8 --notes-file - < doc/notes-v0.1.8.md
   ```
   - gh 全路径（PATH 常未含）；`--notes-file -` + stdin 绕开 MSYS 路径坑（`/d/...` 形式 gh 不认）。
   - notes 草稿放 `doc/`（gitignore，不签入）。
   - 若 GitHub 自动生成了只含 `Full Changelog` 的空 Release，同样用 `gh release edit` 覆盖 body。

## 签名密钥

- 私钥：`doc/updater.key`（gitignore，**本机唯一副本**）；公钥：`doc/updater.key.pub`，已写入 `tauri.conf.json` 的 `plugins.updater.pubkey`。
- `TAURI_SIGNING_PRIVATE_KEY` env 接受私钥**内容**（非路径）：本地手动构建用 `TAURI_SIGNING_PRIVATE_KEY="$(< doc/updater.key)"`。
- GitHub secret `TAURI_SIGNING_PRIVATE_KEY` 设后**不可读回**。
- **丢失 `doc/updater.key` → 必须重新生成密钥 + 所有已装用户重装**。建议额外备份到密码管理器。

## 产物与 updater

- 构建产物：`src-tauri/target/release/bundle/nsis/ITSM Manager_<ver>_x64-setup.exe` + `.sig`。
- Release 自动含 `latest.json`（Tauri updater 检查用）。
- updater endpoints：`https://github.com/SIE-Operations-and-Maintenance-Team/ITSM-Manager/releases/latest/download/latest.json`。
- 应用内自更新：启动时拉 latest.json 比对版本，有新版下载 NSIS + 校验 `.sig` + passive 安装。

## 当前版本参照

- 代码版本（三处）：`0.1.7`
- 最近已发布：`0.1.6`（见 `doc/latest.json`）
- `0.1.7` 尚未发布（无对应 tag/Release）
