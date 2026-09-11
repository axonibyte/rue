# Testing a hook

## Unit-test the handlers

`Hooks.answer(request)` is the whole dispatch: it takes one request frame
as a `Map` and returns the reply the serve loop would write. Tests need no
process and no daemon:

```java
@Test
void anEntryItCannotRecordIsRefused(@TempDir Path dir) {
    Hooks hooks = AuditHook.hooks(dir.resolve("no-such-dir").resolve("audit.ndjson"));
    Map<String, Object> reply = hooks.answer(Map.of(
            "id", 1L, "kind", "journal", "op", "append", "entry", Map.of("seq", 1L)));
    assertEquals(false, reply.get("ok"));
    assertTrue(String.valueOf(reply.get("error")).contains("is not writable"));
}
```

`src/test/java/dev/rue/hook/example/AuditHookTest.java` in this library
tests the quick start this way. Test the refusals as carefully as the
answers: a refusal's text is what the operator reads when a plan stops.

## Drive it over stdio

A hook is a process reading lines, so the shell can drive one: its first
line is the registration, it waits for `{"register":{"ok":true}}`, and it
then answers each request line. The README shows a session. Your own output
must go to stderr; anything else on stdout is a line the engine cannot
read.

## Judge it with `rue sdk-conform`

`rue sdk-conform` starts a hook the way `rued` does, checks its
registration, and then sends every op of every kind it registered,
checking each reply against the protocol (the id comes back, `ok` is a
boolean, an `ok: true` carries every required field, and it arrives within
the deadline) and against the answer [sdk-conformance.md] scripts for it:

```text
$ rue sdk-conform --name audit "java -cp target/classes dev.rue.hook.example.AuditHook"
ok      registration :: the first line is a registration this protocol admits
        serves journal, notify
ok      journal.append :: an entry is acknowledged
        acknowledged
ok      notify.deliver :: a notification is acknowledged
        acknowledged
ok      execute.reboot :: an op this protocol has no row for is refused, never met with silence
        refused by name

4 passed, 0 failed, against the protocol of docs/hook-protocol.md v1
```

For `journal` and `notify` the scripted answer is an acknowledgement, so a
hook of your own passes as it stands. The other kinds' cases expect the
answers of the conformance world (its hosts, its probes, its proofs), and
a hook that is not `dev.rue.hook.example.ConformanceHook` fails them by
design; they judge the library, not your hook's behavior.

## Run a daemon in dry-run mode

`rued run --dry-run` needs no executors and turns every apply into a
rehearsal, which makes it a safe way to see your hook registered and
journaling:

```sh
RUE_AUDIT_LOG=$PWD/audit.ndjson rued run --dry-run --site site.rue \
  --store ./store --socket $PWD/rued.sock --group "$(id -gn)" \
  --spawn audit="java -cp target/classes dev.rue.hook.example.AuditHook"
```

The first line in `audit.ndjson` is the daemon's record of the hook
registering (`hook_registered`), delivered through the hook itself.

## The library's own tests

`sh sdk/test-all.sh java`, from the repository root, runs this library's
JUnit suite (`mvn test`; JUnit is a test-scope dependency and reaches no
consumer of the jar), and `sh sdk/conform-all.sh java` judges its
conformance hook. Both run in rue's pipeline and on its Ubuntu test guest.

[sdk-conformance.md]: ../../../docs/sdk-conformance.md
