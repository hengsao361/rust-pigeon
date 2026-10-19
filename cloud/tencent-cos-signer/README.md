# 腾讯云 COS 上传签名函数

此函数为 Rust Pigeon 客户端生成 5 分钟有效的 COS `PUT` 预签名地址。客户端不会保存腾讯云密钥。

固定配置：

- Bucket：`pg-1329186100`
- Region：`ap-nanjing`
- 对象路径：`log/printer/{client_id}/{本地CSV文件名}`

例如：

```text
log/printer/0123456789abcdef0123456789abcdef/print-history-0123456789abcdef0123456789abcdef-2026-10-19.csv
```

同一客户端的当天文件再次上传时覆盖同一对象；如果 Bucket 开启了版本控制，COS 会保留历史版本。

## 1. 创建最小权限运行角色

在腾讯云访问管理中创建一个供云函数使用的角色，并仅附加以下自定义策略：

```json
{
  "version": "2.0",
  "statement": [
    {
      "effect": "allow",
      "action": [
        "name/cos:PutObject"
      ],
      "resource": [
        "qcs::cos:ap-nanjing:uid/1329186100:pg-1329186100/log/printer/*"
      ]
    }
  ]
}
```

不要给角色增加读取、删除、列举 Bucket 或访问其他目录的权限。

## 2. 创建并部署云函数

1. 使用 Node.js 18.15 创建事件函数，入口设置为 `index.main_handler`。
2. 为函数绑定上一步的运行角色。SCF 会在运行时注入临时的 `TENCENTCLOUD_SECRETID`、`TENCENTCLOUD_SECRETKEY` 和 `TENCENTCLOUD_SESSIONTOKEN`。
3. 在本目录执行 `npm install --omit=dev`，将 `index.js`、`package.json` 和 `node_modules` 一起压缩上传；也可以使用控制台的在线依赖安装。
4. 配置环境变量 `UPLOAD_TOKEN`，值至少 32 个字符，且每套部署使用独立随机值。
5. 可选环境变量：`COS_BUCKET`、`COS_REGION`、`COS_OBJECT_PREFIX`、`URL_EXPIRES_SECONDS`。代码中的默认值已经对应本项目；有效期会限制在 60～900 秒。
6. 在函数详情中打开“函数 URL”，启用公网访问，授权类型选择“开放（NONE）”，不需要启用 CORS。记录形如 `https://<app-id>-<url-id>.ap-nanjing.tencentscf.com` 的 HTTPS 地址。不要创建 API 网关触发器。

这里选择“开放”只表示 SCF 平台不要求客户端进行腾讯云 CAM/TC3 签名；函数代码仍会强制验证 `Authorization: Bearer <UPLOAD_TOKEN>`，缺少或错误的 Token 会返回 `401`。如果选择“CAM 鉴权”，Windows 客户端还需要持有腾讯云调用凭证，与本项目“不向客户端分发云密钥”的安全设计冲突。

签名函数只接受下列请求，并校验客户端标识、文件名归属和 UTC 日期：

```json
{
  "client_id": "0123456789abcdef0123456789abcdef",
  "file_name": "print-history-0123456789abcdef0123456789abcdef-2026-10-19.csv"
}
```

## 3. 配置构建密钥

客户电脑不需要配置环境变量。函数 URL 已作为 HTTPS 地址内置在客户端；签名服务 Token 不写入 Git，而是在 GitHub Actions 构建时从仓库 Secret 注入 EXE。

1. 打开 GitHub 仓库的 `Settings > Secrets and variables > Actions`。
2. 新建 Repository secret，名称为 `RUST_PIGEON_LOG_UPLOAD_TOKEN`。
3. 值必须与本函数的环境变量 `UPLOAD_TOKEN` 完全相同。
4. 重新运行 Windows build；非 PR 构建会校验该 Secret 至少包含 32 个字符。

本地打包时，可以只在当前 PowerShell 进程中临时设置同名环境变量，再运行 `scripts\package-windows.ps1`；它不需要写进客户电脑的用户环境变量。

关闭软件时，客户端先请求一个对象专用的短期上传地址，再直接将 CSV `PUT` 到 COS。失败的文件保留在本地，并在下次关闭时重试。
