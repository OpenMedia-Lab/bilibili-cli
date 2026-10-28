# Bilibili CLI design

Rust CLI for the OpenMedia-Lab tool family. The `bilibili` binary imports an
existing Bilibili session from a file or stdin; it does not handle passwords or
create accounts. Browser-cookie, Netscape and biliup JSON formats are accepted.

The web protocol is independent of the official OAuth application platform.
Cookie requests are confined to fixed Bilibili API origins. UPOS receives only
its scoped upload token. Redirects are disabled and errors omit remote bodies,
URLs and credential values. Persistent session/receipt files use mode 0600 on
Unix. The session is kept outside the repository by default.

Read operations: session status, profile, public video details, own archives,
and creator category metadata. Local operations: ffprobe/MP4 validation, SHA-256,
cover validation, metadata validation and submission preview.

Mutations are explicit commands: cover upload; chunked video upload; and submit
with `--publish`. A submission without that flag is an offline preview, not a
remote draft. No undocumented draft API is claimed. Upload receipts make completed
uploads reusable; partial uploads can resume the same UPOS object. Submission
intent is persisted before a single POST and is never automatically retried.
An uncertain response requires manual reconciliation against own archives.

`archive` reads the creator edit snapshot. `edit` binds a replacement upload to
the original AID, checks the current owner and stable snapshot fields, then uses
the edit endpoint once. It retains AID/BVID and uses the replacement upload's CID.
Collection mutations verify ownership and complete section membership; ordering
requires each current episode ID exactly once. All three mutations require
`--publish`, retain a private attempt receipt and never automatically retry.

Tests will exercise credential parsing/redaction/permissions, upload byte ranges
and authentication boundaries against a local HTTP server, interruption/resume,
and the preview/publish and duplicate-submission boundaries. Live public reads
may be tried. Authenticated mutations will not be run for this delivery.
