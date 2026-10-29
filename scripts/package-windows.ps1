param(
    [string]$TesseractDir = "$env:ProgramFiles\Tesseract-OCR",
    [string]$OutputDir = "dist",
    [string]$BuildNumber = $env:GITHUB_RUN_NUMBER,
    [switch]$IncludeTesseract
)

$ErrorActionPreference = "Stop"

$Metadata = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) {
    throw "读取 Cargo 项目信息失败。"
}
$Package = $Metadata.packages | Where-Object { $_.name -eq "rust-pigeon" } | Select-Object -First 1
if (-not $Package) {
    throw "Cargo.toml 中找不到 rust-pigeon 包。"
}

$BaseVersion = $Package.version
$BuildNumber = "$BuildNumber".Trim()
if ($BuildNumber -and $BuildNumber -notmatch "^[0-9]+$") {
    throw "BuildNumber 必须是非负整数，当前值：$BuildNumber"
}

if ($BuildNumber) {
    $DisplayVersion = "$BaseVersion+$BuildNumber"
    $FileVersion = "$BaseVersion-build.$BuildNumber"
} else {
    $DisplayVersion = $BaseVersion
    $FileVersion = $BaseVersion
}

if ($IncludeTesseract) {
    $TesseractPackageVersionFile = Join-Path $PSScriptRoot "..\TESSERACT_VERSION"
    if (-not (Test-Path $TesseractPackageVersionFile)) {
        throw "找不到 Tesseract 固定版本文件 $TesseractPackageVersionFile。"
    }
    $TesseractPackageVersion = (Get-Content $TesseractPackageVersionFile -Raw).Trim()

    if (-not (Test-Path "$TesseractDir\tesseract.exe")) {
        throw "找不到 $TesseractDir\tesseract.exe，请用 -TesseractDir 指定 Tesseract 5 安装目录。"
    }

    if (-not (Test-Path "$TesseractDir\tessdata\eng.traineddata")) {
        throw "找不到英文模型 $TesseractDir\tessdata\eng.traineddata。"
    }

    $TesseractVersionOutput = (& "$TesseractDir\tesseract.exe" --version 2>&1 | Out-String).Trim()
    if ($LASTEXITCODE -ne 0) {
        throw "无法读取 Tesseract 版本：$TesseractVersionOutput"
    }
    $TesseractVersionLine = ($TesseractVersionOutput -split "`r?`n")[0]
    if ($TesseractVersionLine -notmatch "tesseract\s+v?(\d+)\.(\d+)\.(\d+)") {
        throw "无法解析 Tesseract 版本：$TesseractVersionLine"
    }
    $TesseractMajor = [int]$Matches[1]
    $TesseractMinor = [int]$Matches[2]
    if ($TesseractMajor -lt 5 -or ($TesseractMajor -eq 5 -and $TesseractMinor -lt 2)) {
        throw "Tesseract 版本必须不低于 5.2，当前为：$TesseractVersionLine"
    }
}

$env:RUST_PIGEON_BUILD_VERSION = $DisplayVersion
cargo build --release --locked
if ($LASTEXITCODE -ne 0) {
    throw "Rust release 构建失败。"
}

New-Item -ItemType Directory -Force $OutputDir | Out-Null
$ExeName = "rust-pigeon-v$FileVersion.exe"

if ($IncludeTesseract) {
    $PackageName = "rust-pigeon-windows-x64-v$FileVersion"
    $StagingDir = Join-Path $OutputDir $PackageName
    if (Test-Path $StagingDir) {
        Remove-Item -Recurse -Force $StagingDir
    }
    New-Item -ItemType Directory -Force $StagingDir | Out-Null

    $OcrDir = Join-Path $StagingDir "tesseract"
    $TessdataDir = Join-Path $OcrDir "tessdata"
    New-Item -ItemType Directory -Force $OcrDir | Out-Null
    New-Item -ItemType Directory -Force $TessdataDir | Out-Null

    Copy-Item "target\release\rust-pigeon.exe" (Join-Path $StagingDir $ExeName) -Force
    Copy-Item "$TesseractDir\tesseract.exe" $OcrDir -Force
    Copy-Item "$TesseractDir\*.dll" $OcrDir -Force
    Copy-Item "$TesseractDir\tessdata\eng.traineddata" $TessdataDir -Force
    Set-Content -Path (Join-Path $OcrDir "PACKAGE_VERSION.txt") -Value $TesseractPackageVersion -Encoding utf8
    Set-Content -Path (Join-Path $OcrDir "VERSION.txt") -Value $TesseractVersionOutput -Encoding utf8
    Get-ChildItem $TesseractDir -File | Where-Object {
        $_.Name -match "^(LICENSE|COPYING|NOTICE)"
    } | Copy-Item -Destination $OcrDir -Force

    $PackagePath = Join-Path $OutputDir "$PackageName.zip"
    Compress-Archive -Path "$StagingDir\*" -DestinationPath $PackagePath -Force
    Remove-Item -Recurse -Force $StagingDir
    Write-Host "已生成 $PackagePath（版本 $DisplayVersion，包含 Tesseract）"
} else {
    $PackagePath = Join-Path $OutputDir $ExeName
    Copy-Item "target\release\rust-pigeon.exe" $PackagePath -Force
    Write-Host "已生成 $PackagePath（版本 $DisplayVersion，不包含 Tesseract）"
}

