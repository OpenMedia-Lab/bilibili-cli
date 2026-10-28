# Revise an existing video and manage its collection

[README](../README.md) · [Command guide](usage.md) · [中文操作手册](usage.zh-CN.md)

`edit` updates an existing archive. `collection add` appends a published video to
an existing section; `collection order` changes its episode order. All default to
preview and require `--publish` to write. Collection previews make authenticated
reads; edit previews are entirely offline.

## Replace an existing video

Upload the new MP4 and cover using the [normal upload commands](usage.md#uploads-and-receipts).
Use a new upload receipt: replacements need the CID recorded by version 0.2.0 or
later. Older complete receipts still work for new submissions but lack this CID.
Prepare complete metadata for the new version, including title, tags, description,
cover and category; omitted optional copy fields use their documented defaults.

```sh
# Replace ORIGINAL_BVID with your video's existing BVID. Keep snapshots private.
umask 077
bilibili archive ORIGINAL_BVID > ./before.json

bilibili edit --snapshot ./before.json --metadata ./submission.json \
  --upload ./replacement.receipt.json

# Only after reviewing the new video, cover and exact payload:
bilibili edit --snapshot ./before.json --metadata ./submission.json \
  --upload ./replacement.receipt.json --publish --result ./edit.receipt.json

bilibili archive ORIGINAL_BVID > ./after.json
bilibili readback ./edit.receipt.json --public
```

The plan retains the original AID/BVID and contains the replacement CID and
uploaded filename. Before sending it, the CLI checks the current account,
replacement permission and a fingerprint of the original archive's content and
settings. A concurrent change requires a fresh snapshot and another review.

This command supports an ordinary, already published **single-part** archive and
one replacement upload. It refuses multipart, paid, interactive, staff and premiere
archives. Existing explicit visibility, Dolby/lossless and recreation settings are
retained. Bilibili's creator interface locks the creation declaration after
publication: omit `creation_statement_id` to preserve it, or supply the same ID.
Changing it is rejected locally. For a new submission, select the current option
from `bilibili statements` (on the verification date, ID `1` is “含AI生成内容”).
The archive's AID/BVID remain the target; the video part gets a
new CID. The CLI does not promise migration of CID-specific features such as
danmaku.

Acceptance does not prove that transcoding or moderation has completed. Compare
the fresh creator snapshot's AID/BVID, video CID/filename, copy and state with the
plan. If it is in a collection, also read the section and verify its membership,
episode order and CID after processing. A public detail read alone may still show
the previous version.

## Collections

Read your collection and section IDs before planning a change:

```sh
bilibili collection list --page 1 --page-size 20
bilibili collection show SEASON_ID
bilibili collection section SECTION_ID

# Preview a new, already published video joining an existing section:
bilibili collection add --season SEASON_ID --section SECTION_ID --video NEW_BVID

# Execute the reviewed addition, then read the section back:
bilibili collection add --season SEASON_ID --section SECTION_ID --video NEW_BVID \
  --publish --result ./collection-add.receipt.json
bilibili collection section SECTION_ID
```

Substitute real numeric IDs for `SEASON_ID` and `SECTION_ID`. The add command
checks collection/video ownership and the video's processing state. Existing
membership in that section returns `already_member` without writing. It refuses
moving a video from another section/collection, paid content and multipart videos.

To order episodes, use their **episode IDs**, not AIDs, CIDs or BVIDs:

```sh
bilibili collection order --season SEASON_ID --section SECTION_ID \
  --episodes EPISODE_1,EPISODE_2,EPISODE_3

# Add --publish --result ./collection-order.receipt.json after review.
```

Supply every current episode exactly once in the desired order. An incomplete
section response, duplicate ID, omitted episode or unexpected membership fails
before writing. Collection creation, deletion, section moves and paid collections
are outside this command surface.

## Receipts and recovery

Each mutation writes a private attempt receipt before sending one request. A
successful response records `accepted`; any unsuccessful or ambiguous response
records `uncertain`. Neither is automatically retried. Reusing an existing result
path is blocked; omitting `--result` derives a private path from the plan hash.
Changing a path or payload bypasses this local guard and is not recovery.

Reconcile the original archive or section before deciding what to do next. A
receipt can establish which operation was attempted; only current creator/public
state establishes which version or collection order is available.

## Verification — 2026-10-28

- Live read-only checks: creator archive edit details, replacement permission,
  collection list/detail, section membership and order using the user's session.
- Protocols: current official creator and uploader JavaScript; sources are listed
  in [provenance](../THIRD_PARTY_NOTICES.md). The sort payload uses `sort`, while
  the section read reports `order`.
- Local HTTP tests: edit identity/new CID, stale snapshot rejection, offline edit
  preview, authenticated collection preflight, full-order validation, ownership
  and completeness rejection, existing-member no-op, empty successful responses,
  uncertain/duplicate-attempt guards, and recreation switch serialization. The
  archive read model exposes a recreation object; the editor sends its integer
  switch, matching the official uploader's contract.
- Separately authorized live verification: two single-part videos were replaced
  in place, retaining their AID/BVID and collection episode order. Creator and
  public reads confirmed the new CIDs and approved metadata. One new public video
  was appended as the collection's third episode. These observations verify this
  workflow; collection reordering remains covered by contract tests only.

## 中文要点

`edit` 保留原 AID/BVID，替换素材会使用新的 CID，不删除或重发。先用 `archive`
保存私有快照，再上传新素材；默认预览完全离线，加 `--publish` 才提交原位修改。
提交前会校验账号、替换权限和快照是否过期。稿件处于审核或处理阶段时先等待。
平台在投稿后锁定原生创作声明；原位编辑保留旧声明，不能承诺改成新的 AI 声明。
新投稿从 `bilibili statements` 选择当前声明，2026-10-28 的「含AI生成内容」ID 为 `1`。

`collection add` 把已公开的普通单 P 视频加入已有免费合集；已有成员直接返回，
不会重复添加。`collection order` 使用 episode ID，并要求完整列出当前全部成员。
这两个预览需要联网读取当前状态；只有 `--publish` 才写入。

请求被接收不等于新版本已公开。回读原稿件和合集，核对新 CID、文案、状态和顺序。
出现 `uncertain` 时先核对远端；不要换回执路径盲目重试。
