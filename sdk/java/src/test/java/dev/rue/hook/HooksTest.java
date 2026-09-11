package dev.rue.hook;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.Test;

/** Dispatch: every request gets a reply carrying its id, and every refusal says why. */
class HooksTest {

    private static Map<String, Object> req(String kind, String op, Object... kv) {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("id", 42L);
        m.put("kind", kind);
        m.put("op", op);
        for (int i = 0; i < kv.length; i += 2) {
            m.put((String) kv[i], kv[i + 1]);
        }
        return m;
    }

    @Test
    void anOpTheProtocolDoesNotHaveIsRefusedByName() {
        Map<String, Object> r = new Hooks().answer(req("execute", "teleport"));
        assertEquals(42L, r.get("id"));
        assertEquals(false, r.get("ok"));
        assertTrue(String.valueOf(r.get("error")).contains("execute.teleport"), String.valueOf(r));
        assertEquals(false, new Hooks().answer(req("weather", "report")).get("ok"));
    }

    @Test
    void anOpOfAKindThisHookDoesNotServeIsRefusedNotSilent() {
        Map<String, Object> r = new Hooks().answer(req("journal", "append", "entry", Map.of()));
        assertEquals(42L, r.get("id"));
        assertEquals(false, r.get("ok"));
        assertTrue(String.valueOf(r.get("error")).contains("does not serve journal.append"), String.valueOf(r));
    }

    @Test
    void aHandlersRefusalAndAHandlersFaultBothBecomeReasons() {
        Hooks h = new Hooks();
        h.notify = (level, subject, body) -> {
            if (level.equals("warn")) throw new Hooks.Refusal("paging is off tonight");
            throw new IllegalStateException("socket closed");
        };
        Map<String, Object> refused = h.answer(req("notify", "deliver", "level", "warn"));
        assertEquals("paging is off tonight", refused.get("error"));
        Map<String, Object> fault = h.answer(req("notify", "deliver", "level", "err"));
        assertEquals(false, fault.get("ok"));
        assertTrue(String.valueOf(fault.get("error")).contains("IllegalStateException: socket closed"));
    }

    @Test
    void aReplyMissingARequiredFieldIsRefusedHereNotSentForR0303() {
        Hooks h = new Hooks();
        h.approval = new Hooks.Approval() {
            public List<Object> authenticators() {
                return List.of();
            }

            public String challenge(String i, String d, Object s, Object c) {
                return "c";
            }

            public Map<String, Object> verify(String i, String d, Object s, String a, String p) {
                return Map.of("reason", "forgot the verdict");
            }
        };
        Map<String, Object> r = h.answer(req("approval", "verify"));
        assertEquals(false, r.get("ok"));
        assertTrue(String.valueOf(r.get("error")).contains("without verified"), String.valueOf(r));
    }

    @Test
    void readFactsNoSuchFileIsAnAnswerWithNoContent() {
        Hooks h = new Hooks();
        h.execute = new Hooks.Execute() {
            public Map<String, Object> run(String host, String instance, List<Object> body) {
                return Hooks.output("", Map.of());
            }

            public String readFact(String host, String shape) {
                return shape.endsWith("present") ? "" : null;
            }
        };
        Map<String, Object> absent = h.answer(req("execute", "read_fact", "shape", "file:/absent"));
        assertEquals(true, absent.get("ok"));
        assertFalse(absent.containsKey("content"));
        Map<String, Object> empty = h.answer(req("execute", "read_fact", "shape", "file:/present"));
        assertEquals("", empty.get("content"), "an empty file is not an absent one");
    }

    @Test
    void theRegistrationNamesOnlyTheKindsSupplied() {
        Hooks h = new Hooks();
        h.probe = (host, probe) -> Hooks.observation("yes", "");
        h.notify = (level, subject, body) -> {};
        Map<String, Object> reg = Serve.registration("x", h);
        assertEquals(List.of("probe", "notify"), reg.get("kinds"));
        assertEquals(Op.HOOK_PROTOCOL, reg.get("protocol"));
    }
}
