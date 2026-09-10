package dev.rue.hook.example;

import dev.rue.hook.Hooks;
import dev.rue.hook.Json;
import dev.rue.hook.Serve;

import java.io.BufferedReader;
import java.io.InputStreamReader;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * The reference conformance hook for the Java SDK.
 *
 * <p>docs/sdk-conformance.md's fixed world, served over stdio: what
 * {@code rue sdk-conform} is pointed at to judge this SDK, and the worked
 * example a host application copies.
 *
 * <p>The exception is the four provocations of the {@code probe} kind, which
 * deliberately violate the protocol. Those cannot go through
 * {@code Hooks.answer}, because it builds replies from the op's own row and
 * an {@code ok: true} without a required field is not expressible -- that is
 * the guarantee the SDK exists for. So this file drops to the wire for
 * exactly those, and for nothing else.
 */
public final class ConformanceHook {

    private static final List<String> PROVOCATIONS =
            List.of("conform-missing-field", "conform-no-ok", "conform-silent");

    /** Every kind over the contract's fixed world, deliberately stateless. */
    static final class World
            implements Hooks.Journal, Hooks.Inventory, Hooks.Execute, Hooks.Probe,
                    Hooks.Approval, Hooks.Secrets, Hooks.Scheduler {

        @Override
        public void append(Object entry) {}

        @Override
        public List<Object> list() {
            Map<String, Object> full = new LinkedHashMap<>();
            full.put("name", "conform-full");
            full.put("address", "198.51.100.7");
            full.put("os", "freebsd");
            full.put("roles", List.of("a", "b"));
            full.put("reach", List.of("hook"));
            full.put("filesystem", true);
            full.put("stdin_preamble", false);
            full.put("scheduler", "cron");
            full.put("rue_root", "/var/db/rue");
            full.put("artifact", "python");
            full.put("facts", Map.of("site", "west"));
            // Only what Appendix C requires; the rest takes its default.
            Map<String, Object> bare = new LinkedHashMap<>();
            bare.put("name", "conform-bare");
            bare.put("os", "linux");
            return List.of(full, bare);
        }

        @Override
        @SuppressWarnings("unchecked")
        public Map<String, Object> run(String host, String instance, List<Object> body) {
            Map<String, Object> prim = (Map<String, Object>) ((Map<String, Object>) body.get(0)).get("run");
            String cmd = Hooks.expose(prim.get("cmd"));
            String pw = null;
            for (Object pair : (List<Object>) prim.getOrDefault("env", List.of())) {
                List<Object> kv = (List<Object>) pair;
                if ("PW".equals(kv.get(0))) {
                    pw = Hooks.expose(kv.get(1));
                }
            }
            if (pw == null) {
                throw new Hooks.Refusal("the contract's run carries PW");
            }
            Map<String, Object> outputs = new LinkedHashMap<>();
            outputs.put("echo", cmd);
            // `execute.run` carries a secret in both directions (7.5). An
            // SDK that scrubbed it on the way in, or could not reach it,
            // fails here.
            outputs.put("secret", pw);
            return Hooks.output("ran " + body.size() + " primitive\n", outputs);
        }

        @Override
        public String readFact(String host, String shape) {
            return "file:/conformance/present".equals(shape) ? "present\n" : null;
        }

        @Override
        public Map<String, Object> bootstrapState(String host) {
            Map<String, Object> m = new LinkedHashMap<>();
            m.put("rue_root", true);
            m.put("group", true);
            m.put("instances_dir", true);
            m.put("lock", true);
            m.put("modes_ok", true);
            return m;
        }

        @Override
        public long clock(String host) {
            return 1700000000L;
        }

        @Override
        public void instanceDirCreate(String host, String instance) {}

        @Override
        public void instanceDirRemove(String host, String instance) {}

        @Override
        public List<Object> instanceDirList(String host) {
            Map<String, Object> d = new LinkedHashMap<>();
            d.put("instance", "conform-1");
            d.put("armed", true);
            d.put("fired", false);
            d.put("modes_ok", true);
            return List.of(d);
        }

        @Override
        public void putFile(String h, String i, String rel, String content, long mode) {}

        @Override
        public void replaceFile(String h, String i, String rel, String content) {}

        @Override
        public String getFile(String h, String i, String rel) {
            return "1700000000\n";
        }

        @Override
        public void removeFile(String h, String i, String rel) {}

        @Override
        public void hostLock(String host) {}

        @Override
        public Map<String, Object> observe(String host, String probe) {
            return switch (probe) {
                case "conform-yes" -> Hooks.observation("yes", "yes");
                case "conform-no" -> Hooks.observation("no", "no");
                case "conform-unknown" -> Hooks.observation("unknown", "");
                case "conform-refuse" -> throw new Hooks.Refusal(
                        "refused as the conformance contract asks, with a reason to read");
                default -> throw new Hooks.Refusal("no probe named " + probe);
            };
        }

        @Override
        public List<Object> authenticators() {
            return List.of(
                    Hooks.authenticator("conform-human", true),
                    Hooks.authenticator("conform-machine", false));
        }

        @Override
        public String challenge(String instance, String digest, Object scope, Object context) {
            return "approve " + digest + " on " + instance + " (" + scopeText(scope) + ")";
        }

        @Override
        public Map<String, Object> verify(
                String instance, String digest, Object scope, String authenticator, String proof) {
            // Bound to the digest *and* the scope (5.11). Built from what the
            // request carries, never from anything remembered, which is what
            // makes a replay fail.
            String want = digest + "/" + scopeText(scope);
            return want.equals(proof)
                    ? Hooks.verdict(true, "")
                    : Hooks.verdict(false, "the proof was made for another request or another scope");
        }

        @Override
        public String resolve(String reference) {
            return "conformance-resolved-secret";
        }

        @Override
        public Map<String, Object> deliver(String instance, String label, String value) {
            // Declining is an answer, not a refusal: the engine offers the
            // secret to the next acceptor.
            return Hooks.delivery(!"unwanted".equals(label), "receipt-" + label);
        }

        @Override
        public void install(String host, String artifact) {}

        @Override
        public void arm(String host, String artifact, Object deadline) {}

        @Override
        public void rearm(String host, String artifact, Object deadline) {}

        @Override
        public void disarm(String host, String artifact) {}

        @Override
        public Object present(String host, String artifact) {
            if ("conform-present.sh".equals(artifact)) {
                return true;
            }
            if ("conform-absent.sh".equals(artifact)) {
                return false;
            }
            // Never guess: the engine reads false as "install it again".
            return "unknown";
        }
    }

    /** `plan`, `step/<n>`, `ack/<n>` -- the scope as the contract spells it. */
    @SuppressWarnings("unchecked")
    static String scopeText(Object scope) {
        if ("plan".equals(scope)) {
            return "plan";
        }
        if (scope instanceof Map<?, ?> m) {
            Map<String, Object> s = (Map<String, Object>) m;
            if (s.containsKey("step")) {
                return "step/" + s.get("step");
            }
            if (s.containsKey("ack")) {
                return "ack/" + s.get("ack");
            }
        }
        return "unknown";
    }

    /** notify's deliver collides with secrets', so it gets its own object. */
    static final class Notifier implements Hooks.Notify {
        @Override
        public void deliver(String level, String subject, String body) {}
    }

    public static void main(String[] args) throws Exception {
        String name = args.length > 0 ? args[0] : "conform";
        World world = new World();
        Hooks hooks = new Hooks();
        hooks.journal = world;
        hooks.inventory = world;
        hooks.execute = world;
        hooks.probe = world;
        hooks.approval = world;
        hooks.secrets = world;
        hooks.notify = new Notifier();
        hooks.scheduler = world;
        hooks.filesystem = true;
        hooks.stdinPreamble = true;

        BufferedReader in =
                new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
        PrintStream out = new PrintStream(System.out, true, StandardCharsets.UTF_8);
        out.println(Json.write(Map.of("register", Serve.registration(name, hooks))));
        String ack = in.readLine();
        if (ack == null) {
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
            Object probe = frame.get("probe");
            if ("probe".equals(frame.get("kind")) && probe != null
                    && PROVOCATIONS.contains(String.valueOf(probe))) {
                Object id = frame.get("id");
                // Deliberately malformed, and deliberately not through the SDK.
                switch (String.valueOf(probe)) {
                    case "conform-missing-field" -> {
                        Map<String, Object> m = new LinkedHashMap<>();
                        m.put("id", id);
                        m.put("ok", true);
                        out.println(Json.write(m));
                    }
                    case "conform-no-ok" -> {
                        Map<String, Object> m = new LinkedHashMap<>();
                        m.put("id", id);
                        out.println(Json.write(m));
                    }
                    // conform-silent: say nothing at all.
                    default -> { }
                }
                continue;
            }
            out.println(Json.write(hooks.answer(frame)));
        }
    }
}
