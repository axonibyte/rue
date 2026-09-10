package dev.rue.hook;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * An interface per kind, and the dispatch from a request frame to a handler.
 *
 * <p>The registration frame is built from the kinds actually supplied, so a
 * hook cannot register for one it does not serve -- the failure that produces
 * is a plan binding to it and refusing at its first step, a long way from
 * where the mistake was made.
 *
 * <p>A handler returns its answer or throws {@link Refusal}. A refusal is a
 * hook saying no, on the record, and is not a fault; any other exception
 * becomes one too, because an unanswered request is Silent and Silent tells
 * the operator nothing about why.
 */
public final class Hooks {

    /** Why a hook will not answer: the text the engine journals. */
    public static class Refusal extends RuntimeException {
        public Refusal(String why) {
            super(why);
        }

        public static Refusal unserved(String kind, String op) {
            return new Refusal("this hook does not serve " + kind + "." + op);
        }
    }

    public interface Journal {
        void append(Object entry);
    }

    public interface Inventory {
        List<Object> list();
    }

    /** The instance-directory ops below {@code run} are required only of a
     * hook that registers {@code filesystem} (7.7). */
    public interface Execute {
        Map<String, Object> run(String host, String instance, List<Object> body);

        /** The file's text, or null for no such file -- an answer, not a refusal. */
        default String readFact(String host, String shape) {
            throw Refusal.unserved("execute", "read_fact");
        }

        default Map<String, Object> bootstrapState(String host) {
            throw Refusal.unserved("execute", "bootstrap_state");
        }

        /** The one op the engine reads a refusal of as "no skew probe is
         * possible here" rather than as a fault. */
        default long clock(String host) {
            throw Refusal.unserved("execute", "clock");
        }

        default void instanceDirCreate(String host, String instance) {
            throw Refusal.unserved("execute", "instance_dir_create");
        }

        default void instanceDirRemove(String host, String instance) {
            throw Refusal.unserved("execute", "instance_dir_remove");
        }

        default List<Object> instanceDirList(String host) {
            throw Refusal.unserved("execute", "instance_dir_list");
        }

        default void putFile(String host, String instance, String rel, String content, long mode) {
            throw Refusal.unserved("execute", "put_file");
        }

        default void replaceFile(String host, String instance, String rel, String content) {
            throw Refusal.unserved("execute", "replace_file");
        }

        default String getFile(String host, String instance, String rel) {
            throw Refusal.unserved("execute", "get_file");
        }

        default void removeFile(String host, String instance, String rel) {
            throw Refusal.unserved("execute", "remove_file");
        }

        default void hostLock(String host) {
            throw Refusal.unserved("execute", "host_lock");
        }
    }

    public interface Probe {
        /** Use {@link #observation}. */
        Map<String, Object> observe(String host, String probe);
    }

    public interface Approval {
        List<Object> authenticators();

        String challenge(String instance, String digest, Object scope, Object context);

        /** {@code {verified, reason}}; use {@link #verdict}. */
        Map<String, Object> verify(
                String instance, String digest, Object scope, String authenticator, String proof);
    }

    public interface Secrets {
        default String resolve(String reference) {
            throw Refusal.unserved("secrets", "resolve");
        }

        /** {@code {accepted, receipt}}; use {@link #delivery}. */
        default Map<String, Object> deliver(String instance, String label, String value) {
            throw Refusal.unserved("secrets", "deliver");
        }
    }

    public interface Notify {
        void deliver(String level, String subject, String body);
    }

    public interface Scheduler {
        void install(String host, String artifact);

        void arm(String host, String artifact, Object deadline);

        void rearm(String host, String artifact, Object deadline);

        void disarm(String host, String artifact);

        /** true, false, or the string "unknown". Never guess: the engine
         * reads false as "install it again". */
        Object present(String host, String artifact);
    }

    // --- little constructors, so a handler never hand-builds a wire map ---

    public static Map<String, Object> observation(String tri, String text) {
        return Map.of("text", text, "tri", tri);
    }

    public static Map<String, Object> verdict(boolean verified, String reason) {
        return Map.of("verified", verified, "reason", reason);
    }

    public static Map<String, Object> delivery(boolean accepted, String receipt) {
        return Map.of("accepted", accepted, "receipt", receipt);
    }

    public static Map<String, Object> authenticator(String id, boolean human) {
        return Map.of("id", id, "human", human);
    }

    public static Map<String, Object> output(String stdout, Map<String, Object> outputs) {
        return Map.of("stdout", stdout, "outputs", outputs);
    }

    /** The text of a resolved value. Named rather than reached through the
     * map so that reading a secret is a visible act in the code that does it. */
    @SuppressWarnings("unchecked")
    public static String expose(Object resolved) {
        if (resolved instanceof Map<?, ?> m) {
            return String.valueOf(((Map<String, Object>) m).get("text"));
        }
        return String.valueOf(resolved);
    }

    // --- the hooks themselves --------------------------------------------

    public Journal journal;
    public Inventory inventory;
    public Execute execute;
    public Probe probe;
    public Approval approval;
    public Secrets secrets;
    public Notify notify;
    public Scheduler scheduler;
    /** This hook serves the instance-directory ops (7.7). */
    public boolean filesystem;
    public boolean stdinPreamble;

    public List<String> kinds() {
        List<String> k = new ArrayList<>();
        if (journal != null) k.add("journal");
        if (inventory != null) k.add("inventory");
        if (execute != null) k.add("execute");
        if (probe != null) k.add("probe");
        if (approval != null) k.add("approval");
        if (secrets != null) k.add("secrets");
        if (notify != null) k.add("notify");
        if (scheduler != null) k.add("scheduler");
        return k;
    }

    /**
     * Answer one request frame. The reply always carries the request's id,
     * including for an op with no handler: a request left unanswered is
     * Silent, and the engine can only report that as "the step did not
     * happen".
     */
    public Map<String, Object> answer(Map<String, Object> request) {
        Object id = request.get("id");
        String kind = str(request, "kind");
        String opName = str(request, "op");
        Op row = Op.find(kind, opName);
        if (row == null) {
            return refusal(id, kind + "." + opName + " is not an op of this protocol");
        }
        Map<String, Object> fields;
        try {
            fields = dispatch(row, request);
        } catch (Refusal why) {
            return refusal(id, why.getMessage());
        } catch (RuntimeException e) {
            return refusal(id, e.getClass().getSimpleName() + ": " + e.getMessage());
        }
        Map<String, Object> reply = new LinkedHashMap<>();
        reply.put("id", id);
        reply.put("ok", true);
        reply.putAll(fields);
        for (String f : row.requiredReply()) {
            if (!reply.containsKey(f)) {
                // Replies are built from the op's row, so this is a bug here
                // and not an R0303 for the far end to puzzle over.
                return refusal(id, "the SDK built a " + kind + "." + opName + " reply without " + f);
            }
        }
        return reply;
    }

    private static Map<String, Object> refusal(Object id, String why) {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("id", id);
        m.put("ok", false);
        m.put("error", why);
        return m;
    }

    private static String str(Map<String, Object> m, String k) {
        Object v = m.get(k);
        return v == null ? "" : String.valueOf(v);
    }

    private static Map<String, Object> one(String k, Object v) {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put(k, v);
        return m;
    }

    @SuppressWarnings("unchecked")
    private Map<String, Object> dispatch(Op row, Map<String, Object> r) {
        String host = str(r, "host");
        String inst = str(r, "instance");
        switch (row.kind()) {
            case "journal" -> {
                if (journal == null) throw Refusal.unserved(row.kind(), row.op());
                Object entry = r.get("entry");
                if (entry == null) throw new Refusal("journal.append without an entry");
                journal.append(entry);
                return Map.of();
            }
            case "inventory" -> {
                if (inventory == null) throw Refusal.unserved(row.kind(), row.op());
                return one("hosts", inventory.list());
            }
            case "probe" -> {
                if (probe == null) throw Refusal.unserved(row.kind(), row.op());
                return one("fact", probe.observe(host, str(r, "probe")));
            }
            case "notify" -> {
                if (notify == null) throw Refusal.unserved(row.kind(), row.op());
                notify.deliver(str(r, "level"), str(r, "subject"), str(r, "body"));
                return Map.of();
            }
            case "approval" -> {
                if (approval == null) throw Refusal.unserved(row.kind(), row.op());
                return switch (row.op()) {
                    case "authenticators" -> one("authenticators", approval.authenticators());
                    case "challenge" -> one("challenge",
                            approval.challenge(inst, str(r, "digest"), r.get("scope"), r.get("context")));
                    default -> approval.verify(inst, str(r, "digest"), r.get("scope"),
                            str(r, "authenticator"), str(r, "proof"));
                };
            }
            case "secrets" -> {
                if (secrets == null) throw Refusal.unserved(row.kind(), row.op());
                if (row.op().equals("resolve")) {
                    return one("value", secrets.resolve(str(r, "ref")));
                }
                return secrets.deliver(inst, str(r, "label"), str(r, "value"));
            }
            case "scheduler" -> {
                if (scheduler == null) throw Refusal.unserved(row.kind(), row.op());
                Object deadline = r.get("deadline");
                String artifact = str(r, "artifact");
                switch (row.op()) {
                    case "install" -> scheduler.install(host, artifact);
                    case "arm" -> scheduler.arm(host, artifact, deadline);
                    case "rearm" -> scheduler.rearm(host, artifact, deadline);
                    case "disarm" -> scheduler.disarm(host, artifact);
                    default -> {
                        return one("present", scheduler.present(host, artifact));
                    }
                }
                return Map.of();
            }
            default -> {
                if (execute == null) throw Refusal.unserved(row.kind(), row.op());
                switch (row.op()) {
                    case "run" -> {
                        List<Object> body = (List<Object>) r.getOrDefault("body", List.of());
                        return one("output", execute.run(host, inst, body));
                    }
                    case "read_fact" -> {
                        String c = execute.readFact(host, str(r, "shape"));
                        // null is "no such file", which is an answer; an
                        // empty string would be a file that exists and is
                        // empty.
                        return c == null ? Map.of() : one("content", c);
                    }
                    case "bootstrap_state" -> {
                        return one("state", execute.bootstrapState(host));
                    }
                    case "clock" -> {
                        return one("epoch_s", execute.clock(host));
                    }
                    case "instance_dir_create" -> execute.instanceDirCreate(host, inst);
                    case "instance_dir_remove" -> execute.instanceDirRemove(host, inst);
                    case "instance_dir_list" -> {
                        return one("dirs", execute.instanceDirList(host));
                    }
                    case "put_file" -> execute.putFile(host, inst, str(r, "rel"), str(r, "content"),
                            r.get("mode") instanceof Long l ? l : 0L);
                    case "replace_file" -> execute.replaceFile(host, inst, str(r, "rel"), str(r, "content"));
                    case "get_file" -> {
                        return one("content", execute.getFile(host, inst, str(r, "rel")));
                    }
                    case "remove_file" -> execute.removeFile(host, inst, str(r, "rel"));
                    default -> execute.hostLock(host);
                }
                return Map.of();
            }
        }
    }
}
