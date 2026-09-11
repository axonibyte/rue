# 0013: Python SDK: the reply-shape check is an assert

- status: open
- kind: defect
- phase: 5
- opened: 2026-09-11

`rue_hook.Hooks.answer` checks that a reply carries every field its op
requires with `assert not missing`. Under `python -O` the check is
stripped, and when it does fire the `AssertionError` escapes `answer` and
ends the serve loop; every other SDK refuses such a reply by name. It is
unreachable today -- the dispatch always supplies the fields -- which is
why it is low, and a test cannot reach it without breaking the dispatch.
`serve_socket` also builds a `hello` dict it never sends.
