package dev.rue.hook;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.io.BufferedReader;
import java.io.ByteArrayOutputStream;
import java.io.PrintStream;
import java.io.StringReader;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.Test;

/** The loop: one reply per request line, nothing for anything else, and a budget. */
class ServeTest {

    private static List<Map<String, Object>> serve(String input, Hooks hooks, Duration budget) {
        ByteArrayOutputStream bytes = new ByteArrayOutputStream();
        PrintStream out = new PrintStream(bytes, true, StandardCharsets.UTF_8);
        Serve.serve(new BufferedReader(new StringReader(input)), out, hooks, budget);
        List<Map<String, Object>> replies = new ArrayList<>();
        for (String line : bytes.toString(StandardCharsets.UTF_8).split("\n")) {
            if (!line.isBlank()) replies.add(Json.parseObject(line));
        }
        return replies;
    }

    private static Hooks probe(long sleepMs) {
        Hooks h = new Hooks();
        h.probe = (host, p) -> {
            try {
                Thread.sleep(sleepMs);
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
            }
            return Hooks.observation("yes", "ok");
        };
        return h;
    }

    @Test
    void onlyRequestsAreAnsweredAndEachOnce() {
        String input = String.join("\n",
                "",
                "not json at all",
                "[\"an\",\"array\"]",
                "{\"event\":{\"plan\":\"p\"}}",
                "{\"id\":1}",
                "{\"id\":2,\"kind\":\"probe\",\"op\":\"observe\",\"host\":\"h\",\"probe\":\"up\"}",
                "[" .repeat(50_000),
                "{\"id\":3,\"kind\":\"probe\",\"op\":\"observe\",\"host\":\"h\",\"probe\":\"up\"}");
        List<Map<String, Object>> replies = serve(input, probe(0), null);
        assertEquals(2, replies.size(), String.valueOf(replies));
        assertEquals(2L, replies.get(0).get("id"));
        assertEquals(3L, replies.get(1).get("id"), "a pathological line did not end the loop");
    }

    @Test
    void aHandlerOverItsBudgetAnswersNoAndSaysWhy() {
        String line = "{\"id\":9,\"kind\":\"probe\",\"op\":\"observe\",\"host\":\"h\",\"probe\":\"up\"}\n";
        Map<String, Object> slow = serve(line, probe(150), Duration.ofMillis(20)).get(0);
        assertEquals(9L, slow.get("id"));
        assertEquals(false, slow.get("ok"));
        assertTrue(String.valueOf(slow.get("error")).contains("budget"), String.valueOf(slow));
        Map<String, Object> quick = serve(line, probe(0), Duration.ofSeconds(5)).get(0);
        assertEquals(true, quick.get("ok"));
    }

    @Test
    void aReplyThatCannotBeWrittenBecomesARefusalNotADeadHook() {
        Hooks h = new Hooks();
        h.probe = (host, p) -> Map.of("text", "", "tri", Double.NaN);
        String input = "{\"id\":5,\"kind\":\"probe\",\"op\":\"observe\",\"host\":\"h\",\"probe\":\"x\"}\n"
                + "{\"id\":6,\"kind\":\"probe\",\"op\":\"observe\",\"host\":\"h\",\"probe\":\"x\"}\n";
        List<Map<String, Object>> replies = serve(input, h, null);
        assertEquals(2, replies.size(), String.valueOf(replies));
        assertEquals(false, replies.get(0).get("ok"));
        assertEquals(6L, replies.get(1).get("id"));
    }
}
