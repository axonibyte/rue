# 0012: The Rust conformance hook ends at a line that is not JSON

- status: open
- kind: defect
- phase: 5
- opened: 2026-09-11

`sdk/rust/src/bin/conform_hook.rs` reads with `read_frame(...)?`, so a
line that is not JSON ends it -- the defect the SDK's serve loop and the
shim had until the docs unit fixed them. It is a fixture only `rue
sdk-conform` drives, and the runner never sends such a line, so nothing is
broken today; but the file is the worked example the other SDKs'
conformance hooks copy. Low.
