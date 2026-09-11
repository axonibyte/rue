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
