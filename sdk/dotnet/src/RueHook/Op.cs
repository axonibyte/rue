namespace Rue.Hook;

/// <summary>
/// The hook protocol's wire, as data (docs/hook-protocol.md v1).
///
/// A transcription of <c>hook-proto/src/op.rs</c>, which is the protocol's
/// one source. <c>tools/lint-hook-ops.sh</c>, a gate phase, checks this
/// against that source in both directions, so the two cannot drift.
/// </summary>
public sealed record Op(
    string Kind,
    string OpName,
    string[] Request,
    /// Fields an `ok: true` reply must carry; R0303 without them.
    string[] RequiredReply,
    string[] OptionalReply,
    /// "to_hook" or "to_engine" for the four messages of 7.5 that may
    /// carry a secret; null for every other.
    string? Secret,
    /// True only for `execute.clock`, which a hook may decline outright.
    bool Optional)
{
    public const int HookProtocol = 1;

    /// The eight kinds a hook may register, in the order 7.5 lists them.
    public static readonly string[] Kinds =
    {
        "journal", "inventory", "execute", "probe", "approval", "secrets", "notify", "scheduler"
    };

    // A row with no optional reply, no secret and not optional. Named `Row`
    // so the table is greppable as data and no other call spells the same.
    private static Op Row(string kind, string op, string[] request, string[] required) =>
        new(kind, op, request, required, Array.Empty<string>(), null, false);

    public static readonly Op[] Ops =
    {
        Row("journal", "append", new[] { "entry" }, Array.Empty<string>()),
        Row("inventory", "list", Array.Empty<string>(), new[] { "hosts" }),
        new Op("execute", "run", new[] { "host", "instance", "body", "env", "secrets" },
            new[] { "output" }, new[] { "facts" }, "to_hook", false),
        new Op("execute", "read_fact", new[] { "host", "shape" },
            Array.Empty<string>(), new[] { "content" }, null, false),
        Row("execute", "bootstrap_state", new[] { "host" }, new[] { "state" }),
        new Op("execute", "clock", new[] { "host" },
            new[] { "epoch_s" }, Array.Empty<string>(), null, true),
        Row("execute", "instance_dir_create", new[] { "host", "instance" }, Array.Empty<string>()),
        Row("execute", "instance_dir_remove", new[] { "host", "instance" }, Array.Empty<string>()),
        Row("execute", "instance_dir_list", new[] { "host" }, new[] { "dirs" }),
        Row("execute", "put_file", new[] { "host", "instance", "rel", "content", "mode" },
            Array.Empty<string>()),
        Row("execute", "replace_file", new[] { "host", "instance", "rel", "content" },
            Array.Empty<string>()),
        Row("execute", "get_file", new[] { "host", "instance", "rel" }, new[] { "content" }),
        Row("execute", "remove_file", new[] { "host", "instance", "rel" }, Array.Empty<string>()),
        Row("execute", "host_lock", new[] { "host" }, Array.Empty<string>()),
        Row("probe", "observe", new[] { "host", "probe" }, new[] { "fact" }),
        Row("approval", "authenticators", Array.Empty<string>(), new[] { "authenticators" }),
        Row("approval", "challenge", new[] { "instance", "digest", "scope", "context" },
            new[] { "challenge" }),
        new Op("approval", "verify",
            new[] { "instance", "digest", "scope", "authenticator", "proof" },
            new[] { "verified" }, new[] { "reason" }, null, false),
        new Op("secrets", "resolve", new[] { "ref" },
            new[] { "value" }, Array.Empty<string>(), "to_engine", false),
        new Op("secrets", "deliver", new[] { "instance", "label", "value" },
            new[] { "accepted" }, new[] { "receipt" }, "to_hook", false),
        Row("notify", "deliver", new[] { "level", "subject", "body" }, Array.Empty<string>()),
        Row("scheduler", "install", new[] { "host", "artifact", "deadline" }, Array.Empty<string>()),
        Row("scheduler", "arm", new[] { "host", "artifact", "deadline" }, Array.Empty<string>()),
        Row("scheduler", "rearm", new[] { "host", "artifact", "deadline" }, Array.Empty<string>()),
        Row("scheduler", "disarm", new[] { "host", "artifact", "deadline" }, Array.Empty<string>()),
        Row("scheduler", "present", new[] { "host", "artifact", "deadline" }, new[] { "present" })
    };

    /// The op by kind and name, or null for a pair the protocol has no row for.
    public static Op? Find(string kind, string op) =>
        Ops.FirstOrDefault(o => o.Kind == kind && o.OpName == op);
}
