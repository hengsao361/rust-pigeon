param(
    [string]$TesseractDir = "$env:ProgramFiles\Tesseract-OCR",
    [string]$OutputDir = "dist\rust-pigeon-windows-x64"
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path "$TesseractDir\tesseract.exe")) {
    throw "找不到 $TesseractDir\tesseract.exe，请用 -TesseractDir 指定 Tesseract 5 安装目录。"
}

if (-not (Test-Path "$TesseractDir\tessdata\eng.traineddata")) {
    throw "找不到英文模型 $TesseractDir\tessdata\eng.traineddata。"
}

cargo build --release --locked

New-Item -ItemType Directory -Force $OutputDir | Out-Null
$OcrDir = Join-Path $OutputDir "tesseract"
$TessdataDir = Join-Path $OcrDir "tessdata"
New-Item -ItemType Directory -Force $OcrDir | Out-Null
New-Item -ItemType Directory -Force $TessdataDir | Out-Null

Copy-Item "target\release\rust-pigeon.exe" $OutputDir -Force
Copy-Item "$TesseractDir\tesseract.exe" $OcrDir -Force
Copy-Item "$TesseractDir\*.dll" $OcrDir -Force
Copy-Item "$TesseractDir\tessdata\eng.traineddata" $TessdataDir -Force
Get-ChildItem $TesseractDir -File | Where-Object {
    $_.Name -match "^(LICENSE|COPYING|NOTICE)"
} | Copy-Item -Destination $OcrDir -Force

$Archive = "$OutputDir.zip"
Compress-Archive -Path "$OutputDir\*" -DestinationPath $Archive -Force
Write-Host "已生成 $Archive"

