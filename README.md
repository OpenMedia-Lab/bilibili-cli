# bilibili-cli

OpenMedia-Lab 的原生 Rust Bilibili CLI。读取视频和自己的稿件、校验本地 MP4、上传封面与视频，并在明确指定 `--publish` 后提交稿件。

**默认 `submit` 只生成本地预览，不创建 B 站远端草稿。** 视频上传与稿件提交是两个独立动作。本项目使用已有账号的 Web Cookie，不是 Bilibili 官方 SDK，也未接入需要应用资质的开放平台 OAuth API。

## 安装

需要当前稳定版 Rust；本地视频检查另需 FFmpeg 的 `ffprobe`。

```sh
git clone https://github.com/OpenMedia-Lab/bilibili-cli.git
cd bilibili-cli
cargo install --path . --locked
bilibili --help
```

输出为 JSON，进度与错误写入 stderr，成功返回 0、错误返回 1，命令行参数错误返回 2。

## 先在本地审阅

无需登录，也不会进行任何网络请求：

```sh
bilibili video check /path/to/video.mp4
bilibili cover check /path/to/cover.png
cp examples/submission.json /path/to/submission.json
bilibili plan --metadata /path/to/submission.json --video /path/to/video.mp4
```

`video check` 验证 `.mp4` 扩展名、MP4 文件头、ffprobe 容器与视频流、有效时长，并计算 SHA-256；不转码、不渲染。它不能保证平台接受所有编码或内容。封面检查识别 JPEG/PNG 文件头与尺寸，采用本地 10 MiB 文件上限；平台还会执行自己的校验。封面 URL 支持平台返回的 hdslb.com / biliimg.com 及其子域。

## 扫码登录与已有账号会话

```sh
bilibili auth login --qr-file ./bilibili-login.svg
# 使用 Bilibili App 扫码并确认；--timeout 默认 180 秒。
```

终端直接显示二维码；可选 SVG 必须是新文件，权限为 0600。扫码与手机确认分开显示，过期/超时不会替换原会话，只有新会话通过账号检查后才保存。二维码由 Bilibili 官方 passport 接口生成，兼容其当前 account.bilibili.com 跳转地址。

### 导入浏览器会话


先在 Bilibili 官网正常登录，再导入自己导出的 Cookie 文件。支持单行 `Cookie`、Netscape cookie jar、浏览器 JSON 数组、`{"cookies": {...}}` 及 biliup 的 `cookie_info.cookies`。需要 `SESSDATA` 和 `bili_jct`；带域名的导出仅接受 `bilibili.com` 及其子域。

```sh
bilibili auth import --file /private/path/cookies.json
# 或由密码管理器/受控进程写入 stdin，避免秘密进入命令参数与 shell 历史：
your-secret-provider | bilibili auth import --stdin
bilibili status
bilibili me
```

没有 `--cookie VALUE` 参数，也不读取浏览器数据库或收集密码。默认会话位置是操作系统配置目录下的 `bilibili-cli/session.json`；macOS 通常在 `~/Library/Application Support`，Linux 通常在 `~/.config`。用全局 `--session /private/path/account.json` 选择另一个账号。

持久文件在 Unix 上以 0600 写入，新建私有目录为 0700；读取时拒绝组/其他用户可访问的会话和 receipt。已有文件可用 `chmod 600` 修复权限。文件以明文保存，不是系统钥匙串；Windows 的访问权限需由系统/用户配置。`auth logout` 仅删除本地会话，不注销浏览器账号。会话过期需重新导入。

## 读取

```sh
bilibili video show BV17x411w7KC
bilibili video show 170001
bilibili archives --page 1 --page-size 20
bilibili categories
bilibili statements  # 当前可选的创作声明 ID
```

公开视频详情无需会话。自己的稿件、分区元数据和个人信息需要会话。分区会变化，请从当前 `categories` 响应选择 `tid`，不要直接沿用示例数值；可选 `tid_v2` 用于平台提供的新版分区 ID。

## 明确上传素材

以下命令会向 Bilibili 上传素材，但不会提交稿件：

```sh
bilibili cover upload /path/to/cover.png --receipt /private/path/cover.receipt.json
# 把返回的 URL 填入 submission.json 的 cover 字段。
bilibili video upload /path/to/video.mp4 --receipt /private/path/video.receipt.json
```

同一文件与账号再次使用已完成的 receipt 会复用上传结果，不重传。中断后使用同一 receipt：

```sh
bilibili video upload /path/to/video.mp4 --receipt /private/path/video.receipt.json --resume
bilibili receipt /private/path/video.receipt.json
```

上传采用 `bldsa` 线路、串行和最多 64 MiB 的服务端指定分片，每个分片至多重试三次；本版不提供自动线路测速或并发上传。成功确认的分片及其内容 SHA-256 一起记入 receipt。`--resume` 复用原 UPOS 对象与 upload ID，只跳过哈希仍匹配的分片；不匹配或缺少哈希时覆盖上传同一分片。完整文件 SHA-256/大小、账号必须匹配。完成前校验实际读取的全部内容，完成后移除临时上传令牌。

部分上传的 receipt 含有上传令牌，必须与 Cookie 一样保密；`receipt` 命令只输出安全摘要。令牌过期时不能保证恢复，需人工检查后决定是否重新上传。素材可能占用平台存储；本版不提供删除孤立素材功能。

## 预览，再显式投稿

```sh
# 完全离线；不加载会话，不写远端草稿：
bilibili submit --metadata /path/to/submission.json --upload /private/path/video.receipt.json

# 只有加上 --publish，才会提交到 Bilibili 进行审核/发布：
bilibili submit --metadata /path/to/submission.json --upload /private/path/video.receipt.json --publish
```

`--upload` 可重复，顺序决定分 P 顺序。`part_titles` 可在 metadata 中提供同样长度的标题数组，默认各 P 沿用总标题。支持标题、`tid`/`tid_v2`、标签、简介、动态、封面、原创/转载及来源、禁止转载；未知 metadata 字段会拒绝，防止拼写错误被静默忽略。原创 `copyright=1`，转载 `copyright=2` 且必须提供 `source`。通过 `bilibili statements` 读取当前创作声明选项，在 metadata 中设置 `creation_statement_id`；CLI 将其写入官方 Web 客户端使用的 `creation_statement: {id: ...}`。不要根据旧示例猜测声明 ID。当前不支持定时发布、编辑/删除已投稿件、合集、合作投稿及其他高级字段。

发布请求前写入并同步一份投稿 receipt，默认按账号与完整 payload 的 SHA-256 存放在 OS 本地数据目录的 `bilibili-cli/submissions` 下。`--result /private/path/publish.receipt.json` 可指定位置。同一 receipt 存在时拒绝再试；不要通过换路径绕过它。该保护不是平台级幂等键，修改 metadata 或更换 result 路径会绕开原记录。

发布请求只发一次，**不自动重试**。成功只表示平台接收，不代表审核通过或已公开。超时、非成功返回或异常响应保留 `uncertain` 状态；此时先在自己的稿件列表或官方创作中心按标题和时间核对，避免重复投稿。不会自动删除或覆盖已有投稿记录。

```sh
bilibili readback /private/path/publish.receipt.json --public
bilibili archives --page 1 --page-size 50
```

`readback` 使用同一账号检查创作中心稿件，默认最多读取五页，可用 `--max-pages` 调整到 1..20；`--public` 另检查公开详情。若投稿响应没有返回 AID/BVID，需手工匹配稿件，不能凭“未找到”重发。`.lock` 避免并行操作同一 receipt；崩溃后仅在核对进程与远端状态后手动移除锁文件。

## 可靠性与验证范围

Web 接口是非官方契约，可能因风控、账号权限或平台变更失效。Cookie 仅发往固定的 Bilibili API 域名；UPOS 上传仅携带其专属令牌，限制 HTTPS 与 Bilibili 视频域名。禁用自动重定向，错误不输出远端响应正文、凭据或请求 URL。

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

测试覆盖真实本地 HTTP 交互的分片范围/内容、认证边界、恢复流程、投稿预览/重复保护、权限与会话导入。初次交付实际验证了公开详情读取；未使用真实账号上传封面、视频或投稿。完整记录见 [docs/verification.md](docs/verification.md)。

协议来源及许可证见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。项目采用 [MIT License](LICENSE)。
