package dev.rue.hook;

import java.io.BufferedReader;
import java.io.InputStreamReader;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
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
        BufferedReader in =
                new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
        PrintStream out = new PrintStream(System.out, true, StandardCharsets.UTF_8);

        out.println(Json.write(Map.of("register", registration(name, hooks))));
        String ack = in.readLine();
        if (ack == null || !acknowledged(ack)) {
            System.err.println("rue-hook: registration was not acknowledged: " + ack);
            System.exit(1);
        }
        String line;
        while ((line = in.readLine()) != null) {
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
            if (!frame.containsKey("kind")) {
                continue;
            }
            // println on an autoflushing stream: an unflushed reply is a
            // silence, and a silence is a refusal with nothing to say.
            out.println(Json.write(hooks.answer(frame)));
        }
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
