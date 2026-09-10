package dev.rue.hook;

import java.util.List;
import java.util.Map;
import java.util.stream.Collectors;

/**
 * The hook protocol's wire, as data (docs/hook-protocol.md v1).
 *
 * <p>A transcription of {@code hook-proto/src/op.rs}, which is the protocol's
 * one source. {@code tools/lint-hook-ops.sh}, a gate phase, checks this
 * against that source in both directions, so the two cannot drift.
 */
public record Op(
        String kind,
        String op,
        List<String> request,
        /** Fields an {@code ok: true} reply must carry; R0303 without them. */
        List<String> requiredReply,
        List<String> optionalReply,
        /** "to_hook" or "to_engine" for the four messages of 7.5 that may carry a secret. */
        String secret,
        /** True only for {@code execute.clock}, which a hook may decline. */
        boolean optional) {

    public static final int HOOK_PROTOCOL = 1;

    /** The eight kinds a hook may register, in the order 7.5 lists them. */
    public static final List<String> KINDS = List.of(
            "journal", "inventory", "execute", "probe", "approval", "secrets", "notify", "scheduler");

    /** A row with no optional reply, no secret and not optional. Named
     * `row` rather than `of` so the table is greppable as data:
     * `List.of(...)` appears all over it, and a guard should not have to
     * tell the two apart. */
    private static Op row(String kind, String op, List<String> request, List<String> required) {
        return new Op(kind, op, request, required, List.of(), null, false);
    }

    public static final List<Op> OPS = List.of(
            row("journal", "append", List.of("entry"), List.of()),
            row("inventory", "list", List.of(), List.of("hosts")),
            new Op("execute", "run", List.of("host", "instance", "body", "env", "secrets"),
                    List.of("output"), List.of("facts"), "to_hook", false),
            new Op("execute", "read_fact", List.of("host", "shape"),
                    List.of(), List.of("content"), null, false),
            row("execute", "bootstrap_state", List.of("host"), List.of("state")),
            new Op("execute", "clock", List.of("host"),
                    List.of("epoch_s"), List.of(), null, true),
            row("execute", "instance_dir_create", List.of("host", "instance"), List.of()),
            row("execute", "instance_dir_remove", List.of("host", "instance"), List.of()),
            row("execute", "instance_dir_list", List.of("host"), List.of("dirs")),
            row("execute", "put_file", List.of("host", "instance", "rel", "content", "mode"), List.of()),
            row("execute", "replace_file", List.of("host", "instance", "rel", "content"), List.of()),
            row("execute", "get_file", List.of("host", "instance", "rel"), List.of("content")),
            row("execute", "remove_file", List.of("host", "instance", "rel"), List.of()),
            row("execute", "host_lock", List.of("host"), List.of()),
            row("probe", "observe", List.of("host", "probe"), List.of("fact")),
            row("approval", "authenticators", List.of(), List.of("authenticators")),
            row("approval", "challenge", List.of("instance", "digest", "scope", "context"),
                    List.of("challenge")),
            new Op("approval", "verify",
                    List.of("instance", "digest", "scope", "authenticator", "proof"),
                    List.of("verified"), List.of("reason"), null, false),
            new Op("secrets", "resolve", List.of("ref"),
                    List.of("value"), List.of(), "to_engine", false),
            new Op("secrets", "deliver", List.of("instance", "label", "value"),
                    List.of("accepted"), List.of("receipt"), "to_hook", false),
            row("notify", "deliver", List.of("level", "subject", "body"), List.of()),
            row("scheduler", "install", List.of("host", "artifact", "deadline"), List.of()),
            row("scheduler", "arm", List.of("host", "artifact", "deadline"), List.of()),
            row("scheduler", "rearm", List.of("host", "artifact", "deadline"), List.of()),
            row("scheduler", "disarm", List.of("host", "artifact", "deadline"), List.of()),
            row("scheduler", "present", List.of("host", "artifact", "deadline"), List.of("present")));

    private static final Map<String, Op> BY_NAME =
            OPS.stream().collect(Collectors.toMap(o -> o.kind() + "." + o.op(), o -> o));

    /** The op by kind and name, or null for a pair the protocol has no row for. */
    public static Op find(String kind, String op) {
        return BY_NAME.get(kind + "." + op);
    }
}
