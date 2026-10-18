# Rust Pigeon

Windows 屏幕区域足环号 OCR 监控与佳博 GP-801CE 静默打印客户端。

## 工作流程

1. 选择 Windows 中已安装的佳博打印机队列。
2. 点击“重新框选”，或按全局热键 `Ctrl+Shift+R`，拖动设定捕捉区域。
3. 程序每 500ms 检查区域；画面发生变化时执行本地 OCR。
4. 唯一合法编号连续稳定 3 次后，通过指定打印队列发送 ESC/POS RAW 数据。
5. 打印后自动走纸并执行部分切纸；相同内容不会重复打印。
6. 每次自动打印、再次打印和测试打印成功后，写入本地 JSONL 日志。

程序不实现开机自启，不访问系统默认打印机。关闭程序即停止监控；最小化窗口不会停止监控。

## 足环号规则

- 三段：`20YY-(01..33|JN)-7位数字`
- 两段：`(01..33|JN)-7位数字`
- 一段：`7位数字`

可以从带有“足环号”等标签的 OCR 文本中抽取编号。中文/长横线会统一为 `-`；数字位置会修正 `O → 0`、`I/l → 1`。如果同一画面内出现多个合法编号，为防止误打，本次不会打印。

支持号码跨行显示，例如：

```text
2025-02
0348234

2026
03-0498274
```

会分别组合为 `2025-02-0348234` 和 `2026-03-0498274`，不会误打印为较短的子号码。

## Windows 运行要求

- Windows 10/11 x64。
- 佳博 GP-801CE 已安装官方驱动，并能在“打印机和扫描仪”中正常测试打印。
- Tesseract 5 英文模型。发布包将其固定放在程序旁的 `tesseract` 子目录，不需要配置 PATH，主界面也不显示其内部路径。

所有应用数据统一保存在 `%APPDATA%\RustPigeon`：

- 配置：`%APPDATA%\RustPigeon\config.json`
- 打印日志：`%APPDATA%\RustPigeon\logs\print-history.jsonl`
- 上传状态：`%APPDATA%\RustPigeon\logs\upload-status.log`

旧版本由 eframe 创建的 `%APPDATA%\Rust Pigeon` 窗口状态目录会在启动时安全清理。最后一次成功打印的编号会持久化，因此重启程序不会立即重复打印静止画面中的旧编号。主界面的“再次打印”按钮可以手动重打最近一次成功识别的号码，“打开日志目录”按钮会直接打开上述日志文件夹。

## 显示器与分辨率变化

捕捉区域使用 Windows 虚拟桌面的物理像素坐标，支持副屏位于主屏左侧时产生的负坐标。框选时按实际显示截图与窗口尺寸分别计算横纵缩放，避免单一 DPI 比例造成坐标偏移。

如果框选完成后更改显示器分辨率、缩放比例或显示器排列，原坐标可能已不再对应原内容。程序会暂停监控并提示重新框选，防止截错区域后误打印。

## 关闭时上传打印日志

关闭窗口时，程序会先等待监控线程结束，再尝试上传完整的 `print-history.jsonl`。云端接口尚未绑定到特定服务，通过用户环境变量配置：

```powershell
[Environment]::SetEnvironmentVariable(
  "RUST_PIGEON_LOG_UPLOAD_URL",
  "https://example.com/api/pigeon-print-logs",
  "User"
)
[Environment]::SetEnvironmentVariable(
  "RUST_PIGEON_LOG_UPLOAD_TOKEN",
  "your-bearer-token",
  "User"
)
```

上传请求使用 `POST`、`Content-Type: application/x-ndjson`，正文为完整日志快照，并携带 `X-Rust-Pigeon-Upload-Mode: full-snapshot`。Token 可选；配置后通过 `Authorization: Bearer ...` 发送。未配置上传 URL 时只写本地日志，不产生网络请求；上传连接超时为 3 秒，总超时为 10 秒，失败不会删除本地记录。

## 开发与验证

```powershell
cargo test
cargo clippy --all-targets -- -D warnings
cargo run
```

在 Windows 安装 Tesseract 5 后生成便携压缩包：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\package-windows.ps1
```

如果 Tesseract 不在默认目录：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\package-windows.ps1 `
  -TesseractDir "D:\Tools\Tesseract-OCR"
```

首次在目标设备上应先点击“测试打印并切纸”，确认 GP-801CE 驱动允许 RAW ESC/POS 数据和 `GS V 66 0` 切刀指令，再开启正式监控。

## GitHub Actions 自动打包

仓库包含 `.github/workflows/windows-build.yml`。推送到 `main`/`master`、推送 `v*` 标签、创建 Pull Request，或在 GitHub Actions 页面手动运行工作流时，GitHub 会在 Windows runner 上：

1. 安装稳定版 Rust，运行单元测试和 Clippy。
2. 安装 Windows 版 Tesseract OCR 和英文模型。
3. 调用 `scripts/package-windows.ps1` 生成便携包。
4. 校验 EXE、Tesseract 和 `eng.traineddata` 均已包含。
5. 上传名为 `rust-pigeon-windows-x64` 的构建产物，保留 14 天。

下载方法：进入 GitHub 仓库的 **Actions** 页面，打开成功的 **Windows build** 任务，在页面底部 **Artifacts** 区域下载 `rust-pigeon-windows-x64`。下载得到的 Actions 压缩包中包含正式的 `rust-pigeon-windows-x64.zip`。

CI 产物未经代码签名，首次运行时 Windows SmartScreen 可能提示未知发布者。CI 只能验证构建和文件完整性；发布前仍需在连接 GP-801CE 的 Windows 10/11 实机上验证截图、OCR、RAW 打印和自动切刀。

## 目录结构

- `src/ring.rs`：文本标准化、正式地区码校验和编号抽取。
- `src/monitor.rs`：持续截图、变化检测、稳定门控和成功打印去重。
- `src/ocr.rs`：Tesseract 本地 OCR 与截图预处理。
- `src/platform/windows.rs`：Win32 GDI 截图、打印机枚举、全局热键和 RAW 打印。
- `src/app.rs`：egui 设置窗口与区域框选。

