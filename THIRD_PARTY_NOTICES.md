# Source provenance and licensing

This is an independent Rust implementation. No third-party implementation files
or SDK binaries are vendored. Protocol field names and HTTP endpoint contracts
were checked against the primary sources listed below on 2026-10-25. They are
not claims of official support or a stable public Web API.

## Protocol references

- **Bilibili official platform documentation**:
  <https://open.bilibili.com/doc>. The official application API is distinct from
  the cookie-authenticated Web flow implemented here.
- **Bilibili official C# demo**:
  <https://github.com/bilibili-openplatform/OpenPlatform_CSharpDemo>. Its README
  confirms that platform access and user authorization precede official API use.
  No code was incorporated from this repository.
- **biliup**, commit
  [`6a7ae5cc459d2af29ed90ceecd35590d36055255`](https://github.com/biliup/biliup/tree/6a7ae5cc459d2af29ed90ceecd35590d36055255),
  MIT license, Copyright (c) 2019 ForgQi. The following source files provided
  primary implementation evidence for the Web/UPOS protocol:
  - `crates/biliup/src/uploader/bilibili.rs`: cover form, creator archive/profile
    reads, category preflight, Web submission endpoint and payload shape.
  - `crates/biliup/src/uploader/line.rs`: preupload parameters and bucket fields.
  - `crates/biliup/src/uploader/line/upos.rs`: multipart initialization, byte-range
    query fields, part completion body and filename derivation.
  These contracts informed new code and tests. The upstream source files were not
  copied. Its full license is reproduced below as an attribution courtesy.
- **reqwest** official Rust documentation/source:
  <https://docs.rs/reqwest/0.12.28/reqwest/>, MIT OR Apache-2.0.
  Used for its blocking client, request builders, TLS, sensitive headers, and
  redirect policy. Dependencies are pinned by `Cargo.lock`; each dependency
  remains under its own license.

The archived `biliup-rs` repository was also inspected to locate its maintained
successor. No source from an API-collection mirror, blog post, or unlicensed code
snippet was incorporated.

## biliup MIT attribution

MIT License

Copyright (c) 2019 ForgQi

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

QR authentication was checked against the official passport.bilibili.com generate/poll endpoints on 2026-10-25. The current returned scan URL uses account.bilibili.com. No third-party authentication implementation was copied.
