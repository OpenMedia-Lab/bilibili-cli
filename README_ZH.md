<p align="center">
  <img src="docs/assets/readme-hero.svg" alt="bilibili-cli：读取 JSON、本地检查、上传素材、预览、明确投稿、回读验证" width="100%">
</p>

<h1 align="center">bilibili-cli</h1>

<p align="center">在终端里读取 Bilibili、检查视频并完成投稿的原生 Rust CLI。</p>

<p align="center">
  <a href="#快速开始">快速开始</a> ·
  <a href="#投稿一条视频">投稿视频</a> ·
  <a href="docs/usage.zh-CN.md">操作手册</a> ·
  <a href="README.md">English</a>
</p>

<p align="center">
  <a href="https://github.com/OpenMedia-Lab/bilibili-cli/actions/workflows/ci.yml"><img src="https://github.com/OpenMedia-Lab/bilibili-cli/actions/workflows/ci.yml/badge.svg" alt="Rust 检查"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-2E58DB" alt="MIT 协议"></a>
</p>

用 JSON 读取公开视频详情，上传前先检查 MP4；通过 Bilibili App 扫码登录，保存上传回执，
再审阅完整投稿参数。**`submit` 默认只生成离线预览，加上 `--publish` 才会向 Bilibili 投稿。**

## 快速开始

安装当前稳定版 [Rust](https://rustup.rs/)。检查本地 MP4 时，还需要 [FFmpeg](https://ffmpeg.org/download.html) 中的 `ffprobe`。

```sh
git clone https://github.com/OpenMedia-Lab/bilibili-cli.git
cd bilibili-cli
cargo install --path . --locked

bilibili --version
bilibili video show 170001
```

读取公开视频不需要登录。标准输出为 JSON，进度和错误写入标准错误。
横幅展示了这条只读命令的部分返回字段，核验日期为 2026-10-27。

### 使用自己的账号

```sh
bilibili auth login
# 用 Bilibili App 扫描终端二维码，并在手机上确认。
bilibili status
bilibili archives --page 1 --page-size 20
```

需要图形二维码时，加上 `--qr-file ./bilibili-login.svg`，使用新的文件路径。
已有自己导出的 Cookie？查看[会话导入](docs/usage.zh-CN.md#导入浏览器会话)。

## 选择下一步

| 想做什么 | 从这里开始 |
| --- | --- |
| 读取公开视频详情 | `bilibili video show <BVID-or-AID>` |
| 本地检查视频和封面 | `bilibili video check video.mp4` · `bilibili cover check cover.png` |
| 离线检查稿件参数与媒体 | `bilibili plan --metadata submission.json --video video.mp4` |
| 上传文件，中断后继续 | [上传与回执](docs/usage.zh-CN.md#明确上传素材) |
| 预览、投稿、回读结果 | [投稿流程](#投稿一条视频) |
| 选择当前分区与创作声明 | `bilibili categories` · `bilibili statements` |

## 投稿一条视频

流程分成**本地检查、上传素材、提交稿件**三个阶段。
上传视频得到的是可复用素材；离线预览也不是平台草稿。

**1. 本地准备与检查**

复制 [examples/submission.json](examples/submission.json)，修改标题、标签和简介，
通过 `bilibili categories` 选择当前有效的分区。

```sh
cp examples/submission.json ./submission.json
bilibili plan --metadata ./submission.json --video ./video.mp4
bilibili cover check ./cover.png
```

**2. 上传素材**

```sh
bilibili cover upload ./cover.png --receipt ./cover.receipt.json
# 将返回的封面 URL 填入 submission.json 的 cover 字段。
bilibili video upload ./video.mp4 --receipt ./video.receipt.json
```

**3. 审阅参数，再明确投稿**

```sh
bilibili submit --metadata ./submission.json --upload ./video.receipt.json

# 确认视频、封面与文案后才执行：
bilibili submit --metadata ./submission.json --upload ./video.receipt.json \
  --publish --result ./publish.receipt.json

bilibili readback ./publish.receipt.json --public
```

回执记录素材或稿件的实际状态。上传中断后，使用同一份视频回执并加上 `--resume`。
投稿结果不确定时，先回读核对；平台接收和公开可见是两种状态。
[恢复操作说明 →](docs/usage.zh-CN.md#预览再显式投稿)

分 P 投稿可按顺序重复 `--upload`，并在 metadata 中用 `part_titles` 设置各 P 标题。
[参数说明 →](docs/usage.zh-CN.md#预览再显式投稿)

## 文档导航

| 文档 | 内容 |
| --- | --- |
| [中文操作手册](docs/usage.zh-CN.md) · [English guide](docs/usage.md) | 会话、本地检查、上传、稿件参数与恢复流程 |
| [实现设计](docs/design.md) | 原生接口与实现边界 |
| [初始验证](docs/verification.md) · [扫码验证](docs/qr-login-verification.md) | 带日期的验证记录及证据范围 |
| [第三方说明](THIRD_PARTY_NOTICES.md) | 协议来源与许可证 |

这是使用既有 Web 会话的非官方客户端，不是平台开放 OAuth SDK。Web 接口可能变化。
当前版本不提供已投稿件编辑／删除、定时发布或合集管理。

## 参与开发

欢迎提交可复现的 CLI 问题、范围明确的改进或文档修正。
提交 PR 前运行 `cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`
和 `cargo test --locked`。请勿把凭据和私人回执放进 issue 或提交记录。

[OpenMedia-Lab](https://github.com/OpenMedia-Lab) 出品。
先制作视频？使用 [videodeck](https://github.com/OpenMedia-Lab/videodeck)。
自有代码采用 [MIT 协议](LICENSE)，第三方条款单独列明。
