# QR login verification

Date: 2026-10-25.

Bilibili: native official QR generated, 0600 SVG produced, pending state polled and timeout preserved prior session. New save_verified regression checks invalid profile rejection and valid replacement.

No phone-confirmation login was performed during automated verification. Existing sessions were preserved. Unit/regression tests and challenge generation are separate evidence from complete live authentication.

Regression result: 25 tests passed in the complete working checkout.

Independent review fix: QR output cannot replace any existing file, including the session path. The new regression failed before create-new was used, then passed. Failed/missing account profile keeps old session; verified profile replacement is tested.
