# 0013: Python SDK: the reply-shape check is an assert

- status: closed
- kind: defect
- phase: 5
- opened: 2026-09-11
- closed: 2026-09-11

`rue_hook.Hooks.answer` checks that a reply carries every field its op
requires with `assert not missing`. Under `python -O` the check is
stripped, and when it does fire the `AssertionError` escapes `answer` and
ends the serve loop; every other SDK refuses such a reply by name. It is
unreachable today -- the dispatch always supplies the fields -- which is
why it is low, and a test cannot reach it without breaking the dispatch.
`serve_socket` also builds a `hello` dict it never sends.

**Closed.** Fixed: a reply missing a field its op requires is refused by name ("the SDK built a <kind>.<op> reply without <field>"), as in every other SDK, under -O and without it; serve_socket's unused hello dict is gone. Test: test_a_reply_missing_a_required_field_is_refused_by_name, which drives answer through a dispatch that forgets the field; row sdk-python-missing-field-asserted.
