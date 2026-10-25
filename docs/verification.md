# Initial delivery verification

Checked on 2026-10-25 on macOS (Apple Silicon), using Rust/Cargo 1.98.1.
No real Bilibili account session was imported. No real cover/video upload or
submission was performed.

## Automated checks

- `cargo fmt --check`: passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed without warnings.
- `cargo test --locked`: 20 tests passed, 0 failed.
- `cargo build --release --locked`: passed.
- Top-level and `submit --help`: checked; explicit `--publish` is documented.

Test coverage includes:

- Raw/browser/Netscape/biliup cookie import; foreign domains and header injection;
  sensitive cookie headers; no secret echo; private writes and symlink rejection.
- Offline CLI preview with a nonexistent session; incomplete receipt rejection;
  stdin session import without credential values in stdout/stderr.
- MP4 probe metadata, finite duration, a real video stream, invalid file contents,
  PNG signature/dimensions.
- Local HTTP server contract tests: preupload fields and bldsa route, exact chunk
  byte content and end-exclusive ranges, scoped UPOS auth without web cookies,
  completion payload, token removal on completion, completed receipt reuse,
  same-object resume after an interrupted chunk, untrusted upload origin rejection,
  and source-file mutation preventing finalization. Regression tests restore the
  original file and resume after both a detected hash mismatch and an earlier
  network failure, checking that the reconstructed uploaded bytes are correct.
  A replacement PUT with a lost acknowledgment also invalidates its previous
  stored hash before the request, so the next resume cannot skip uncertain bytes.
- Offline submission preview; single accepted submission with CSRF/JSON contract
  and persisted IDs; HTTP 500 leaves an uncertain receipt and does not retry;
  cover API errors are not accepted merely because the response includes a URL.

Mocks validate the client against the documented/observed protocol, not the
behavior of a real Bilibili account. They cannot prove current account-specific
permissions, anti-abuse behavior, remote token expiry or moderation results.

## Live read-only checks

- `bilibili video show 170001`: returned successful public data for AID `170001`,
  BVID `BV17x411w7KC`. This request sent no session cookies.
- `bilibili status`: returned `session_exists=false`, `is_login=false` for the
  CLI's default session location. No user account details were accessed.

## Not live-tested

Authenticated profile, own archives, creator categories, cover upload, UPOS
upload/resume, actual submission and creator/public readback after submission.
No platform success is claimed for these operations in this delivery. The CLI
implements them against primary upstream source contracts and fails closed on
unrecognized responses.

There is no remote draft implementation, official OAuth application integration,
automatic browser-cookie extraction, QR login, edit/delete, scheduling, collection
management, or advanced creative-declaration support. Preview is a local artifact.
See README for operational limits and recovery after uncertain submissions.
