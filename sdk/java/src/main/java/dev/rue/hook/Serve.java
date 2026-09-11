package dev.rue.hook;

import java.io.BufferedReader;
import java.io.InputStreamReader;
import java.io.PrintStream;
import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.LinkedHashMap;
import java.util.Map;

/**
 * The way {@code rued} reaches a hook it spawned (docs/hook-protocol.md,
 * "A hook over stdio").
 *
 * <p>Send the registration frame, read the acknowledgement, then answer one
 * request per line until stdin closes. Event frames from a subscription are
 * skipped: a hook that is not also an operator has nothing to do with them.
 */
public final class Serve {
    private Serve() {}

    public static Map<String, Object> registration(String name, Hooks hooks) {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("name", name);
        m.put("kinds", hooks.kinds());
        m.put("protocol", Op.HOOK_PROTOCOL);
        m.put("filesystem", hooks.filesystem);
        m.put("stdin_preamble", hooks.stdinPreamble);
        return m;
    }

    /**
     * Serve as a child the daemon spawned ({@code rued run --spawn}).
     *
     * <p>The registration frame is the first line of stdout, before anything
     * else, so keep your own logging on stderr.
     */
    public static void stdio(String name, Hooks hooks) throws Exception {
        stdio(name, hooks, null);
    }

    /**
     * As {@link #stdio(String, Hooks)}, with a budget for one handler. The
     * engine's deadline ({@code rued run --hook-deadline}) is not on the
     * wire, so an SDK cannot see it; what it can do is keep its own slowness
     * from arriving as a silence. A handler that overruns the budget answers
     * {@code ok: false} naming the overrun -- a refusal with a reason is
     * worth more to the operator than a timeout. {@code null} leaves a slow
     * handler to the engine's deadline. The Rust and Python SDKs do the same.
     */
    public static void stdio(String name, Hooks hooks, Duration budget) throws Exception {
        BufferedReader in =
                new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
        PrintStream out = new PrintStream(System.out, true, StandardCharsets.UTF_8);

        out.println(Json.write(Map.of("register", registration(name, hooks))));
        String ack = in.readLine();
        if (ack == null || !acknowledged(ack)) {
            System.err.println("rue-hook: registration was not acknowledged: " + ack);
            System.exit(1);
        }
        serve(in, out, hooks, budget);
    }

    /**
     * Answer one request per line of {@code in} until it ends: the loop
     * {@link #stdio} runs after registering. Blank lines, lines that are not
     * a JSON object, event frames and frames with no {@code kind} get no
     * reply; every request gets exactly one.
     */
    public static void serve(BufferedReader in, PrintStream out, Hooks hooks, Duration budget) {
        String line;
        while ((line = readLine(in)) != null) {
            line = line.trim();
            if (line.isEmpty()) {
                continue;
            }
            Map<String, Object> frame;
            try {
                frame = Json.parseObject(line);
            } catch (RuntimeException e) {
                continue;
            }
            if (frame.containsKey("event") || !frame.containsKey("kind")) {
                continue;
            }
            long started = System.nanoTime();
            Map<String, Object> reply = hooks.answer(frame);
            Duration took = Duration.ofNanos(System.nanoTime() - started);
            if (budget != null && took.compareTo(budget) > 0 && Boolean.TRUE.equals(reply.get("ok"))) {
                reply = refusal(frame.get("id"), "the handler took " + took.toMillis() + "ms, over its "
                        + budget.toMillis() + "ms budget; answering late is worse than answering no");
            }
            String text;
            try {
                text = Json.write(reply);
            } catch (RuntimeException e) {
                // A reply the codec cannot write -- a NaN a handler put in
                // it -- is refused by name rather than ending the loop.
                text = Json.write(refusal(frame.get("id"), "the reply could not be written: " + e.getMessage()));
            }
            // println on an autoflushing stream: an unflushed reply is a
            // silence, and a silence is a refusal with nothing to say.
            out.println(text);
        }
    }

    private static String readLine(BufferedReader in) {
        try {
            return in.readLine();
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
    }

    private static Map<String, Object> refusal(Object id, String why) {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("id", id);
        m.put("ok", false);
        m.put("error", why);
        return m;
    }

    @SuppressWarnings("unchecked")
    private static boolean acknowledged(String line) {
        try {
            Object reg = Json.parseObject(line).get("register");
            return reg instanceof Map<?, ?> m
                    && Boolean.TRUE.equals(((Map<String, Object>) m).get("ok"));
        } catch (RuntimeException e) {
            return false;
        }
    }
}
