# Command guide

[Back to the README](../README.md) · [中文操作手册](usage.zh-CN.md)

Commands below run from the repository root. The executable is `bilibili`.
JSON results go to stdout, diagnostics to stderr. Exit codes are 0 for success,
1 for operation failures and 2 for CLI argument errors.

## Sessions

```sh
bilibili auth login --qr-file ./bilibili-login.svg
bilibili status
bilibili me
```

Scan with the official Bilibili App and confirm on your phone. The terminal also
renders the QR; the SVG is optional and must use a new path. `--timeout` defaults
to 180 seconds. A timeout or expired QR preserves the previous session. The new
session is saved only after account verification.

To import your own exported browser cookies:

```sh
bilibili auth import --file /private/path/cookies.json
# Or send the cookie material through stdin:
your-secret-provider | bilibili auth import --stdin
```

Supported formats are raw Cookie, Netscape cookie jars, browser JSON arrays,
`{"cookies": {...}}`, and biliup's `cookie_info.cookies`. `SESSDATA` and `bili_jct`
are required. Domain-tagged entries must belong to `bilibili.com` or a subdomain.
There is no command-line cookie-value flag, automatic browser-database extraction,
or password collection.

The default session is the OS config directory's `bilibili-cli/session.json`.
Use `--session /private/path/account.json` to select another account. On Unix,
session and receipt files use 0600 permissions; new private directories use 0700.
Reads reject group/other-accessible private files. A session is a plaintext file,
not a keychain entry; Windows file permissions are managed by the OS/user.
`bilibili auth logout` removes only the local session.

## Reads and local checks

```sh
bilibili video show BV17x411w7KC
bilibili archives --page 1 --page-size 20
bilibili categories
bilibili statements

bilibili video check ./video.mp4
bilibili cover check ./cover.png
bilibili plan --metadata ./submission.json --video ./video.mp4
```

Public details need no session. Your archives, account profile, creator categories
and declaration options require one. Read current `categories` and `statements`
rather than copying old category/declaration IDs.

`video check` validates the MP4 extension/header, probes container and video-stream
metadata with ffprobe, checks positive duration, and calculates SHA-256. It does
not transcode, render, or guarantee platform acceptance. `cover check` reads real
JPEG/PNG signatures and dimensions with a local 10 MiB cap. The platform applies
its own additional checks. `plan` is offline and does not load your session.

## Uploads and receipts

These commands upload assets; they do not submit a video:

```sh
bilibili cover upload ./cover.png --receipt ./cover.receipt.json
bilibili video upload ./video.mp4 --receipt ./video.receipt.json

# Continue the same interrupted file/object:
bilibili video upload ./video.mp4 --receipt ./video.receipt.json --resume
bilibili receipt ./video.receipt.json
```

Copy the cover upload's returned URL into metadata. Allowed cover URL domains are
`hdslb.com`, `biliimg.com` and their subdomains. A completed upload receipt can be
reused for the same file and account without retransferring it.

The uploader uses the `bldsa` route and serial server-selected chunks capped at
64 MiB. A chunk has at most three retries. Resume retains the original UPOS object
and upload ID. It skips only acknowledged chunks whose saved SHA-256 still
matches; changed or unhashed chunks are overwritten. Complete-file size/hash and
account must match. Source bytes are checked before finalization, then temporary
upload authorization is removed from a complete receipt.

An incomplete receipt can contain upload authorization. Keep it private like your
cookies; `bilibili receipt` prints a filtered summary. Expired upload authorization
may prevent recovery. There is no concurrent uploader, automatic route benchmark,
or orphaned-asset deletion command.

## Metadata

Start with [examples/submission.json](../examples/submission.json).
Unknown fields are rejected rather than ignored.

| Field | Use |
| --- | --- |
| `title`, `desc`, `dynamic`, `tags` | Title, description, dynamic post and tags |
| `tid`, optional `tid_v2` | Current category IDs from `bilibili categories` |
| `cover` | URL returned by `bilibili cover upload` |
| `copyright`, `source` | `1` for original; `2` for repost, with required source |
| `no_reprint` | Disable reprinting |
| `part_titles` | Optional per-part titles, matching the number of uploads |
| `creation_statement_id` | Current declaration option from `bilibili statements` |

Repeat `--upload` in the desired part order for multipart submissions. The default
part title is the overall title. Declaration IDs are mapped to the web client's
`creation_statement: {id: ...}` field; do not infer a permanent ID from examples.
Scheduling, editing/deleting published submissions, collections and collaborative
submissions are outside this version's command surface.

## Submission and readback

```sh
# Offline payload preview; no session or remote draft:
bilibili submit --metadata ./submission.json --upload ./video.receipt.json

# Actual submission:
bilibili submit --metadata ./submission.json --upload ./video.receipt.json \
  --publish --result ./publish.receipt.json

bilibili readback ./publish.receipt.json --public
bilibili archives --page 1 --page-size 50
```

Before sending a publish request, the CLI writes and syncs a submission receipt.
The default location is the OS local-data directory under
`bilibili-cli/submissions`, keyed by account and payload SHA-256. `--result` selects
an explicit file. An existing submission receipt prevents another attempt using
that file; changing the path or metadata can bypass this local guard and is not a
recovery strategy. It is not a server idempotency key.

A publish request is sent once, with no automatic retry. A successful response
means accepted by the platform, not approved or public. A timeout, unsuccessful
response or unexpected body retains an `uncertain` state. Check the original
receipt and creator state before deciding a next action.

`readback` checks creator archives with the same account, up to five pages by
default; `--max-pages` supports 1–20. `--public` also checks public video details.
If no AID/BVID was returned, match the record manually by title and time; a missing
match does not justify resubmission. A `.lock` prevents concurrent access to a
receipt; remove a stale lock only after reconciling the process and remote state.

## Development and evidence

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

The client uses unofficial web contracts that can change with account permissions,
platform behavior and anti-abuse controls. Session cookies are restricted to fixed
Bilibili API hosts. UPOS transfers use their scoped authorization, HTTPS and
restricted video domains. Automatic redirects are disabled. Errors do not echo
remote response bodies, credentials or request URLs.

[Initial verification](verification.md) and [QR verification](qr-login-verification.md)
are dated records: tests and local HTTP fixtures are distinct from live platform
operations. See [design](design.md), [third-party notices](../THIRD_PARTY_NOTICES.md)
and [MIT license](../LICENSE).
