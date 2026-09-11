package dev.rue.hook;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.Test;

/**
 * 7.11: an SDK exposes execute.run's secrets to the run handler "without
 * ever placing them on a command line". The value has to be reachable --
 * conformance proves that -- and it must not be reachable by accident: a
 * body concatenated into a command, logged, or written back out must not
 * carry the secret's text. This SDK handed handlers the wire map, whose
 * toString printed it.
 */
class ResolvedTest {

    private static final String PW = "correct-horse-battery";

    @Test
    void aSecretIsRedactedWhereverItIsFormatted() {
        Resolved r = new Resolved(PW, true);
        assertFalse(r.toString().contains(PW), r.toString());
        assertFalse(String.valueOf(r).contains(PW));
        assertFalse(("cmd " + r).contains(PW));
        assertFalse(String.format("%s", r).contains(PW));
        assertFalse(Json.write(Map.of("k", r)).contains(PW));
        assertEquals(PW, Hooks.expose(r), "expose is the one way to read it");
    }

    @Test
    void aValueThatIsNotSecretFormatsAsItsText() {
        Resolved r = new Resolved("plain", false);
        assertTrue(r.toString().contains("plain"));
        assertEquals("plain", Hooks.expose(r));
    }

    @Test
    void aRunHandlerReceivesTheBodysSecretsAsResolvedValues() {
        List<Object> seen = new ArrayList<>();
        Hooks hooks = new Hooks();
        hooks.execute = (host, instance, body) -> {
            seen.addAll(body);
            return Hooks.output("", Map.of());
        };
        Map<String, Object> req = new LinkedHashMap<>(Map.of("id", 1L, "kind", "execute", "op", "run",
                "host", "h", "instance", "i"));
        req.put("body", Json.parse("[{\"run\":{\"cmd\":{\"text\":\"deploy\",\"secret\":false},"
                + "\"env\":[[\"PW\",{\"text\":\"" + PW + "\",\"secret\":true}]]}}]"));
        Map<String, Object> reply = hooks.answer(req);
        assertEquals(true, reply.get("ok"), String.valueOf(reply));

        // The whole body, formatted the careless way, carries no secret...
        assertFalse(String.valueOf(seen).contains(PW), String.valueOf(seen));
        // ...and the value is still there for the handler that asks for it.
        @SuppressWarnings("unchecked")
        Map<String, Object> run = (Map<String, Object>) ((Map<String, Object>) seen.get(0)).get("run");
        @SuppressWarnings("unchecked")
        List<Object> pair = (List<Object>) ((List<Object>) run.get("env")).get(0);
        assertInstanceOf(Resolved.class, pair.get(1));
        assertEquals(PW, Hooks.expose(pair.get(1)));
        assertEquals("deploy", Hooks.expose(run.get("cmd")));
    }
}
