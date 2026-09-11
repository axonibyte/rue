package dev.rue.hook.example;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.rue.hook.Json;
import java.io.ByteArrayOutputStream;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

/** AuditHook, the quick start of docs/README.md, does what the page says it does. */
class AuditHookTest {

    @TempDir
    Path dir;

    private static Map<String, Object> req(String kind, String op, Object... kv) {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("id", 1L);
        m.put("kind", kind);
        m.put("op", op);
        for (int i = 0; i < kv.length; i += 2) {
            m.put((String) kv[i], kv[i + 1]);
        }
        return m;
    }

    @Test
    void itRegistersForJournalAndNotify() {
        assertEquals(List.of("journal", "notify"), AuditHook.hooks(dir.resolve("audit.ndjson")).kinds());
    }

    @Test
    void eachEntryIsAppendedAsOneLine() throws Exception {
        Path log = dir.resolve("audit.ndjson");
        var hooks = AuditHook.hooks(log);
        for (long seq : new long[] {1, 2}) {
            Map<String, Object> reply = hooks.answer(req("journal", "append", "entry", Map.of("seq", seq)));
            assertEquals(Map.of("id", 1L, "ok", true), reply);
        }
        List<String> lines = Files.readAllLines(log, StandardCharsets.UTF_8);
        assertEquals(List.of(1L, 2L), lines.stream().map(l -> Json.parseObject(l).get("seq")).toList());
    }

    @Test
    void anEntryItCannotRecordIsRefusedWithTheReason() {
        var hooks = AuditHook.hooks(dir.resolve("no-such-dir").resolve("audit.ndjson"));
        Map<String, Object> reply = hooks.answer(req("journal", "append", "entry", Map.of("seq", 1L)));
        assertEquals(false, reply.get("ok"));
        assertTrue(String.valueOf(reply.get("error")).contains("is not writable"), String.valueOf(reply));
    }

    @Test
    void aNotificationGoesToStderrAndIsAcknowledged() {
        PrintStream was = System.err;
        ByteArrayOutputStream err = new ByteArrayOutputStream();
        System.setErr(new PrintStream(err, true, StandardCharsets.UTF_8));
        Map<String, Object> reply;
        try {
            reply = AuditHook.hooks(dir.resolve("audit.ndjson")).answer(req("notify", "deliver",
                    "level", "warn", "subject", "plan held", "body", "waiting for approval"));
        } finally {
            System.setErr(was);
        }
        assertEquals(Map.of("id", 1L, "ok", true), reply);
        assertEquals("[warn] plan held: waiting for approval" + System.lineSeparator(),
                err.toString(StandardCharsets.UTF_8));
    }
}
