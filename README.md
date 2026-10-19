# Rust Pigeon

Windows 屏幕区域足环号 OCR 监控与佳博 GP-801CE 静默打印客户端。

## 工作流程

1. 选择 Windows 中已安装的佳博打印机队列。
2. 点击“重新框选”，或按全局热键 `Ctrl+Shift+R`；程序会在整个虚拟桌面显示无标题栏的半透明灰色遮罩，直接使用鼠标按下和松开的物理屏幕坐标设定捕捉区域，按 `Esc` 取消。
   监控期间会用亮绿色虚线持续标出捕捉区域；边框置顶且鼠标穿透，暂停监控或重新框选时隐藏。
3. 程序通常每 500ms 检查区域；画面发生变化时执行本地 OCR。
4. 识别到与上次打印不同的唯一合法编号后，第 2、3 次稳定确认加速为每 100ms 一次；连续稳定 3 次后，通过指定打印队列发送 ESC/POS RAW 数据。
5. 打印后自动走纸并执行部分切纸；相同内容不会重复打印。
6. 每次自动打印、再次打印和测试打印成功后，写入按 UTC 日期分割的本地 CSV 日志。

打印内容使用 576 点宽黑白票据位图，包含二维码、足环号、业务内容、“羽成达”和本地时间；底部分割线中间只显示补齐后的当前通道，例如 `05`。票据不依赖热敏打印机内置中文字库。

“稳定确认时重复 OCR”默认关闭：图片指纹相同时，第 2、3 次确认复用第一次 OCR 结果。开启后，新候选打印前会以 100ms 间隔实际执行 3 次 OCR；该选项以 `repeat_ocr_during_stability` 布尔值保存在配置文件中。

“OCR 识别格式”是互斥的单选项，默认“通用”。通用模式同时支持标准环的单行三段、跨行三段、单行两段、单行纯数字 7 位，以及单行纪念环；也可以切换为严格的单行纯数字 6 位、5 位或 4 位。该选项以 `ocr_format` 保存在配置文件中；旧版本的三段、两段和 7 位选项升级后会自动迁移为“通用”。

三段/两段号码在票据中分成上下两行时，上行保留地区码后的横杠，例如 `2026-03-` / `0015369`；纯数字号码不增加横杠。

程序不实现开机自启，不访问系统默认打印机。关闭程序即停止监控；最小化窗口不会停止监控。

## OCR 识别格式

- 通用（默认）：
  - 标准环单行三段：`20YY-(01..33)-7位数字`
  - 标准环跨行三段：最终组合为 `20YY-(01..33)-7位数字`
  - 标准环单行两段：`(01..33)-7位数字`
  - 标准环单行纯数字 7 位
  - 纪念环单行：`(JN|JNH) + 任意两位数字 + - + 6位数字`，例如 `JN05-123456`、`JNH99-654321`
- 单行纯数字 6 位
- 单行纯数字 5 位
- 单行纯数字 4 位

标准环只允许数字地区码 `01–33`，不再接受 `JN` 地区码。可以从带有“足环号”等标签的 OCR 文本中抽取编号。中文/长横线会统一为 `-`；数字位置会修正 `O → 0`、`I/l → 1`。OCR 错误插入到年份（如 `203 1`）、正式地区码（如 `0 1`、`1 1`、`2 1`）或数字号码内部的空格会被安全拼回；组件前后的多余空格也会忽略。修复后的整体仍须满足当前所选格式。如果同一画面内出现多个合法编号，为防止误打，本次不会打印。

通用模式支持标准环三段号码跨行显示，例如：

```text
2025-02
0348234

2026
03-0498274
```

会分别组合为 `2025-02-0348234` 和 `2026-03-0498274`。跨行完整号码优先，不会同时把末行误判成独立的纯 7 位号码；两段标准环和纪念环仍要求单行。

## Windows 运行要求

- Windows 10/11 x64。
- 佳博 GP-801CE 已安装官方驱动，并能在“打印机和扫描仪”中正常测试打印。
- Tesseract 5 英文模型。完整版发布包将其固定放在程序旁的 `tesseract` 子目录，不需要配置 PATH，主界面也不显示其内部路径；默认精简包不包含 Tesseract。

所有应用数据统一保存在 `%APPDATA%\RustPigeon`：

- 配置：`%APPDATA%\RustPigeon\config.json`
- 客户端标识：`%APPDATA%\RustPigeon\client-id.txt`
- 打印日志：`%APPDATA%\RustPigeon\logs\print-history-{client_id}-YYYY-MM-DD.csv`
- 上传状态：`%APPDATA%\RustPigeon\logs\upload-status.log`
- 上传进度：`%APPDATA%\RustPigeon\logs\upload-state.json`

程序首次写日志时生成一个持久化的 32 位十六进制 `client_id`。打印日志按“客户端 + UTC 日期”每天生成一个文件，例如 `print-history-0123456789abcdef0123456789abcdef-2026-10-19.csv`，因此多台电脑同一天的文件名也不会冲突。每个文件第一行为 `channel,timestamp_utc,ring_number,action`，之后每次成功打印追加一行；通道按票据实际使用的两位格式记录。旧版 `print-history.jsonl` 不会删除或自动迁移，新版本不再写入该文件。

旧版本由 eframe 创建的 `%APPDATA%\Rust Pigeon` 窗口状态目录会在启动时安全清理。最后一次成功打印的编号会持久化，因此重启程序不会立即重复打印静止画面中的旧编号。主界面的“再次打印”按钮可以手动重打最近一次成功识别的号码，“打开日志目录”按钮会直接打开上述日志文件夹。

## 二维码票据

主页面可以设置二维码通道和票据顶部的业务内容。通道只接受最多两位数字，默认输入 `0`，写入二维码时补齐为 `00`；例如输入 `5` 会写为 `05`。业务内容每次启动时从“勇夺桂冠、所向披靡、载誉归来、决胜千里、羽翼生风、王者归来”中随机选择一次，本次运行期间保持不变，也可以手动修改。

二维码当前编码格式为：

```text
ycd://足环号&ts=yyyyMMddHHmmss&ch=两位通道&crc=两位大写十六进制校验值
```

其中时间取实际打印时的 Windows 本地时间。“内容”是识别到的完整足环号。CRC 使用 CRC-8/SMBUS 参数：多项式 `0x07`、初始值 `0x00`、输入和输出均不反射、输出异或值 `0x00`；校验范围为 `&crc=` 之前的完整 UTF-8 字节，结果格式化为两位大写十六进制。

## 显示器与分辨率变化

捕捉区域使用 Windows 虚拟桌面的物理像素坐标，支持副屏位于主屏左侧时产生的负坐标。框选时按实际显示截图与窗口尺寸分别计算横纵缩放，避免单一 DPI 比例造成坐标偏移。

如果框选完成后更改显示器分辨率、缩放比例或显示器排列，原坐标可能已不再对应原内容。程序会暂停监控并提示重新框选，防止截错区域后误打印。

## 关闭时上传打印日志

关闭窗口时，程序会先等待监控线程结束，再上传尚未成功上传或成功上传后又发生变化的每日 CSV 文件。每个文件上传成功后立即更新 `upload-state.json`；失败文件会在下次关闭时重试，已经成功且未变化的文件不会重复上传。状态文件损坏时会重新上传每日 CSV，优先避免漏传。

日志上传已经适配腾讯云 COS：客户端通过内置的 HTTPS 函数 URL 申请 5 分钟有效的对象专用 `PUT` 地址，再将 CSV 直接上传到 `pg-1329186100`（`ap-nanjing`）。对象路径为 `log/printer/{client_id}/{本地CSV文件名}`。腾讯云永久密钥只由 SCF 运行角色临时提供，不进入客户端、配置文件或代码仓库。签名函数代码和部署步骤见 [`cloud/tencent-cos-signer`](cloud/tencent-cos-signer/README.md)。

客户电脑不需要配置上传 URL 或环境变量。函数 URL 已固定为 `https://1329186100-0ogxl1fc2j.ap-nanjing.tencentscf.com`；上传 Token 在构建时从 GitHub Actions 仓库 Secret `RUST_PIGEON_LOG_UPLOAD_TOKEN` 注入 EXE，不写入源码。正式分发前必须先配置该 Secret，且它的值必须与云函数环境变量 `UPLOAD_TOKEN` 完全相同。未配置或不足 32 字符时，非 PR 的 CI 构建会直接失败，避免产生无法上传的正式安装包。

客户端以 JSON 将 `client_id` 和 `file_name` 发送给签名函数，并通过 `Authorization: Bearer ...` 鉴权；函数验证文件归属后返回短期 HTTPS 地址。客户端随后使用 `PUT`、`Content-Type: text/csv; charset=utf-8` 上传。失败不会删除本地记录。开发调试时仍可用同名进程环境变量临时覆盖内置 URL 和 Token，但客户无需设置。

## 开发与验证

```powershell
cargo test
cargo clippy --all-targets -- -D warnings
cargo run
```

默认生成不包含 Tesseract 的单个 EXE，不再额外压缩：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\package-windows.ps1
```

精简包适合覆盖升级已经带有 `tesseract` 目录的现有安装；新设备首次部署应使用下面的完整包，否则 OCR 无法运行。

CI 使用 GitHub Actions 的递增 `run_number` 作为构建号。例如基础版本为 `0.1.0`、第 42 次构建时，软件主页面显示 `v0.1.0+42`，精简构建文件名为 `rust-pigeon-v0.1.0-build.42.exe`。本地打包可用 `-BuildNumber 42` 指定同样的构建号；不传且不在 CI 中运行时只使用基础版本。

需要生成包含 Tesseract 的完整包时，先安装 Tesseract 5，然后增加 `-IncludeTesseract`：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\package-windows.ps1 `
  -IncludeTesseract
```

如果 Tesseract 不在默认目录：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\package-windows.ps1 `
  -IncludeTesseract `
  -TesseractDir "D:\Tools\Tesseract-OCR"
```

首次在目标设备上应先点击“测试打印并切纸”，确认 GP-801CE 驱动允许 RAW ESC/POS 数据和 `GS V 66 0` 切刀指令，再开启正式监控。

## GitHub Actions 自动打包

仓库包含 `.github/workflows/windows-build.yml`。推送到 `main`/`master`、推送 `v*` 标签、创建 Pull Request，或在 GitHub Actions 页面手动运行工作流时，GitHub 会在 Windows runner 上：

1. 安装稳定版 Rust，运行单元测试和 Clippy。
2. 使用 GitHub Actions `run_number` 生成递增构建版本，并编译进软件主页面。
3. 默认调用 `scripts/package-windows.ps1`，直接生成带版本号的单个 EXE，不再生成项目内层 ZIP。
4. 手动运行工作流时可勾选 `include_tesseract`，生成包含 Tesseract 和英文模型、文件名带版本号的完整 ZIP。
5. 根据所选模式校验 EXE 或 ZIP 内容，并上传构建产物保留 14 天。

下载方法：进入 GitHub 仓库的 **Actions** 页面，打开成功的 **Windows build** 任务，在页面底部 **Artifacts** 区域下载带版本号的产物。GitHub Actions 自身会把 Artifact 包装成下载用 ZIP；默认构建中不再嵌套第二层 ZIP，解压 Artifact 后直接得到 EXE。

CI 产物未经代码签名，首次运行时 Windows SmartScreen 可能提示未知发布者。CI 只能验证构建和文件完整性；发布前仍需在连接 GP-801CE 的 Windows 10/11 实机上验证截图、OCR、RAW 打印和自动切刀。

Windows EXE 已嵌入与运行时窗口左上角一致的应用图标；资源管理器、桌面快捷方式、任务栏和窗口标题栏会使用同一套多尺寸图标。

## 目录结构

- `src/ring.rs`：文本标准化、正式地区码校验和编号抽取。
- `src/monitor.rs`：持续截图、变化检测、稳定门控和成功打印去重。
- `src/ocr.rs`：Tesseract 本地 OCR 与截图预处理。
- `src/platform/windows.rs`：Win32 GDI 截图、打印机枚举、全局热键和 RAW 打印。
- `src/app.rs`：egui 设置窗口与区域框选。

