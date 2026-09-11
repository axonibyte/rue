# rue-hook for Java

A hook is a process that `rued` calls on a site's behalf: to record journal
entries, run steps on hosts it cannot reach itself, answer probes, approve
gates, resolve and receive secrets, deliver notifications, or schedule
backstops. The protocol is newline-delimited JSON ([hook-protocol.md]).
This library lets you write a hook as an implementation (or a lambda) per
kind, and does the registration handshake, the framing and the reply shapes
for you.

- **No dependencies.** Java 17 or later; the JSON codec is the library's
  own, so the jar is all a host application takes on.
- **Conformance-tested.** `rue sdk-conform` drives every op of every kind
  through this library's own serve loop ([sdk-conformance.md]).
- **Protocol v1**, which is frozen: a hook written against it keeps working
  until a new protocol version says otherwise.

## Install

The artifact is `dev.rue:rue-hook`. It is not on Maven Central; install it
from a checkout of rue into your local repository:

```sh
mvn -f sdk/java/pom.xml install
```

and depend on it:

```xml
<dependency>
  <groupId>dev.rue</groupId>
  <artifactId>rue-hook</artifactId>
  <version>0.2.0</version>
</dependency>
```

Or put `sdk/java/target/rue-hook-0.2.0.jar` (from `mvn package`) on the
classpath.

## Quick start

An audit hook: a journal sink that keeps every entry the engine chains,
and a notifier. This file is
`src/main/java/dev/rue/hook/example/AuditHook.java`, and ships in the jar;
the library's own tests drive it (`AuditHookTest`).

<!-- example: src/main/java/dev/rue/hook/example/AuditHook.java -->
```java
package dev.rue.hook.example;

import dev.rue.hook.Hooks;
import dev.rue.hook.Json;
import dev.rue.hook.Serve;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardOpenOption;

/**
 * An audit hook: a journal sink that keeps every entry, and a notifier.
 *
 * <p>Bind it in a site with {@code journal to: local(), hook(:audit)} and
 * {@code notify via: hook(:audit)}, and have rued spawn it:
 *
 * <pre>rued run --spawn audit="java -cp rue-hook.jar dev.rue.hook.example.AuditHook" ...</pre>
 *
 * <p>Each journal entry is appended to $RUE_AUDIT_LOG (default audit.ndjson)
 * as one line of JSON. A sink that cannot record an entry must say so: the
 * engine then refuses to proceed (R0304) rather than run a step nobody
 * recorded. Notifications go to stderr, because stdout carries the protocol.
 */
public final class AuditHook {
    private AuditHook() {}

    public static Hooks hooks(Path log) {
        Hooks hooks = new Hooks();
        hooks.journal = entry -> {
            try {
                Files.writeString(log, Json.write(entry) + "\n", StandardCharsets.UTF_8,
                        StandardOpenOption.CREATE, StandardOpenOption.APPEND);
            } catch (IOException e) {
                throw new Hooks.Refusal("the audit log " + log + " is not writable: " + e);
            }
        };
        hooks.notify = (level, subject, body) ->
                System.err.println("[" + level + "] " + subject + ": " + body);
        return hooks;
    }

    public static void main(String[] args) throws Exception {
        String log = System.getenv().getOrDefault("RUE_AUDIT_LOG", "audit.ndjson");
        Serve.stdio("audit", hooks(Path.of(log)));
    }
}
```

The protocol is plain lines, so you can drive the hook by hand. After its
registration line it waits for the acknowledgement, then answers one
request per line:

```text
$ mvn -q compile
$ printf '%s\n' '{"register":{"ok":true}}' \
    '{"id":1,"kind":"journal","op":"append","entry":{"seq":1}}' \
    '{"id":2,"kind":"probe","op":"observe","host":"h","probe":"p"}' |
  java -cp target/classes dev.rue.hook.example.AuditHook
{"register":{"name":"audit","kinds":["journal","notify"],"protocol":1,"filesystem":false,"stdin_preamble":false}}
{"id":1,"ok":true}
{"id":2,"ok":false,"error":"this hook does not serve probe.observe"}
```

## Wire it into a site

The site names the hook where it wants it used, and declares who may
register it:

```text
site do
  journal to: local(), hook(:audit)
  notify via: hook(:audit)
  hooks do
    registrar :spawned, user: :socket_owner, may_register: [:audit]
  end
  ...
end
```

Then `rued` starts it as a child and talks to it over its stdin and
stdout:

```sh
rued run --site site.rue --store /var/db/rue --socket /var/run/rue/rued.sock \
  --spawn audit="java -cp /opt/rue/rue-hook-0.2.0.jar dev.rue.hook.example.AuditHook"
```

Four names have to agree: the one the hook registers with (the first
argument of `Serve.stdio`), the `NAME` of `--spawn NAME=COMMAND`, the
`hook(:audit)` the site binds, and one in a registrar's `may_register`.
`rued` refuses a child that registers under any other name. A child `rued`
spawned is the socket owner, so its registrar says `user: :socket_owner`.

## Next

- [guide.md](guide.md): every kind and its interface, refusing, secrets,
  and the budget.
- [testing.md](testing.md): testing a hook, and judging it with
  `rue sdk-conform`.

[hook-protocol.md]: ../../../docs/hook-protocol.md
[sdk-conformance.md]: ../../../docs/sdk-conformance.md
