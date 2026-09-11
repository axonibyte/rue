# 0012: The Rust conformance hook ends at a line that is not JSON

- status: closed
- kind: defect
- phase: 5
- opened: 2026-09-11
- closed: 2026-09-11

`sdk/rust/src/bin/conform_hook.rs` reads with `read_frame(...)?`, so a
line that is not JSON ends it -- the defect the SDK's serve loop and the
shim had until the docs unit fixed them. It is a fixture only `rue
sdk-conform` drives, and the runner never sends such a line, so nothing is
broken today; but the file is the worked example the other SDKs'
conformance hooks copy. Low.

**Closed.** Fixed: the conformance hook's read loop skips a line that is not JSON, as the SDK's serve loop and the shim do, and any other I/O error still ends it. Test: the_conformance_hook_skips_a_line_that_is_not_json (sdk/rust/tests/conform.rs) spawns the real binary; row sdk-rust-conform-hook-bad-line.
