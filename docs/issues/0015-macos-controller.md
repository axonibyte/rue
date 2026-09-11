# 0015: macOS as a controller

- status: open
- kind: not-proven
- phase: -
- opened: 2026-09-11

The darwin binaries are cross-built from Linux with zig and no SDK, and
ship unexecuted and unsigned (ROADMAP 11): executing, smoke-testing,
signing and notarizing them, the launchd daemon, and TCC and Full Disk
Access for the scheduler job wait for a Mac.
