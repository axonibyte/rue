using System.Text.Json;
using System.Text.Json.Nodes;
using Rue.Hook;

namespace Rue.Hook.Example;

/// <summary>
/// The reference conformance hook for the .NET SDK.
///
/// docs/sdk-conformance.md's fixed world, served over stdio: what
/// <c>rue sdk-conform</c> is pointed at to judge this SDK, and the worked
/// example a host application copies.
///
/// The exception is the four provocations of the <c>probe</c> kind, which
/// deliberately violate the protocol. Those cannot go through
/// <c>Hooks.Answer</c>, because it builds replies from the op's own row and
/// an <c>ok: true</c> without a required field is not expressible -- that
/// is the guarantee the SDK exists for. So this file drops to the wire for
/// exactly those, and for nothing else.
/// </summary>
public static class Program
{
    private static readonly string[] Provocations =
        { "conform-missing-field", "conform-no-ok", "conform-silent" };

    /// Every kind over the contract's fixed world, deliberately stateless.
    private sealed class World : IJournal, IInventory, IExecute, IProbe, IApproval, ISecrets, IScheduler
    {
        public void Append(JsonNode entry) { }

        public IEnumerable<object> List() => new object[]
        {
            new Dictionary<string, object?>
            {
                ["name"] = "conform-full",
                ["address"] = "198.51.100.7",
                ["os"] = "freebsd",
                ["roles"] = new[] { "a", "b" },
                ["reach"] = new[] { "hook" },
                ["filesystem"] = true,
                ["stdin_preamble"] = false,
                ["scheduler"] = "cron",
                ["rue_root"] = "/var/db/rue",
                ["artifact"] = "python",
                ["facts"] = new Dictionary<string, object?> { ["site"] = "west" }
            },
            // Only what Appendix C requires; the rest takes its default.
            new Dictionary<string, object?> { ["name"] = "conform-bare", ["os"] = "linux" }
        };

        public object Run(string host, string instance, IReadOnlyList<JsonNode?> body)
        {
            var prim = body[0]?["run"];
            var cmd = Hooks.Expose(prim?["cmd"]);
            string? pw = null;
            if (prim?["env"] is JsonArray env)
            {
                foreach (var pair in env)
                {
                    if (pair is JsonArray kv && kv.Count == 2 &&
                        kv[0]?.GetValue<string>() == "PW")
                    {
                        pw = Hooks.Expose(kv[1]);
                    }
                }
            }

            if (pw is null)
            {
                throw new Refusal("the contract's run carries PW");
            }

            return Hooks.Output($"ran {body.Count} primitive\n", new Dictionary<string, object?>
            {
                ["echo"] = cmd,
                // `execute.run` carries a secret in both directions (7.5).
                // An SDK that scrubbed it on the way in, or could not reach
                // it, fails here.
                ["secret"] = pw
            });
        }

        public string? ReadFact(string host, string shape) =>
            shape == "file:/conformance/present" ? "present\n" : null;

        public object BootstrapState(string host) => new Dictionary<string, object?>
        {
            ["rue_root"] = true,
            ["group"] = true,
            ["instances_dir"] = true,
            ["lock"] = true,
            ["modes_ok"] = true
        };

        public long Clock(string host) => 1700000000L;

        public void InstanceDirCreate(string host, string instance) { }

        public void InstanceDirRemove(string host, string instance) { }

        public IEnumerable<object> InstanceDirList(string host) => new object[]
        {
            new Dictionary<string, object?>
            {
                ["instance"] = "conform-1",
                ["armed"] = true,
                ["fired"] = false,
                ["modes_ok"] = true
            }
        };

        public void PutFile(string h, string i, string rel, string content, long mode) { }

        public void ReplaceFile(string h, string i, string rel, string content) { }

        public string GetFile(string h, string i, string rel) => "1700000000\n";

        public void RemoveFile(string h, string i, string rel) { }

        public void HostLock(string host) { }

        public object Observe(string host, string probe) => probe switch
        {
            "conform-yes" => Hooks.Observation("yes", "yes"),
            "conform-no" => Hooks.Observation("no", "no"),
            "conform-unknown" => Hooks.Observation("unknown", ""),
            "conform-refuse" => throw new Refusal(
                "refused as the conformance contract asks, with a reason to read"),
            _ => throw new Refusal($"no probe named {probe}")
        };

        public IEnumerable<object> Authenticators() => new[]
        {
            Hooks.Authenticator("conform-human", true),
            Hooks.Authenticator("conform-machine", false)
        };

        public string Challenge(string instance, string digest, JsonNode? scope, JsonNode? context) =>
            $"approve {digest} on {instance} ({ScopeText(scope)})";

        public object Verify(
            string instance, string digest, JsonNode? scope, string authenticator, string proof)
        {
            // Bound to the digest *and* the scope (5.11). Built from what
            // the request carries, never from anything remembered, which is
            // what makes a replay fail.
            var want = $"{digest}/{ScopeText(scope)}";
            return proof == want
                ? Hooks.Verdict(true, "")
                : Hooks.Verdict(false, "the proof was made for another request or another scope");
        }

        public string Resolve(string reference) => "conformance-resolved-secret";

        // Declining is an answer, not a refusal: the engine offers the
        // secret to the next acceptor.
        public object Deliver(string instance, string label, string value) =>
            Hooks.Delivery(label != "unwanted", $"receipt-{label}");

        public void Install(string host, string artifact) { }

        public void Arm(string host, string artifact, JsonNode? deadline) { }

        public void Rearm(string host, string artifact, JsonNode? deadline) { }

        public void Disarm(string host, string artifact) { }

        public object Present(string host, string artifact) => artifact switch
        {
            "conform-present.sh" => true,
            "conform-absent.sh" => false,
            // Never guess: the engine reads false as "install it again".
            _ => "unknown"
        };
    }

    /// notify's Deliver collides with secrets', so it gets its own object.
    private sealed class Notifier : INotify
    {
        public void Deliver(string level, string subject, string body) { }
    }

    /// `plan`, `step/&lt;n&gt;`, `ack/&lt;n&gt;` -- the scope as the contract spells it.
    private static string ScopeText(JsonNode? scope)
    {
        if (scope is null)
        {
            return "unknown";
        }

        if (scope.GetValueKind() == JsonValueKind.String && scope.GetValue<string>() == "plan")
        {
            return "plan";
        }

        if (scope["step"] is { } step)
        {
            return $"step/{step.GetValue<long>()}";
        }

        if (scope["ack"] is { } ack)
        {
            return $"ack/{ack.GetValue<long>()}";
        }

        return "unknown";
    }

    public static int Main(string[] args)
    {
        var name = args.Length > 0 ? args[0] : "conform";
        var world = new World();
        var hooks = new Hooks
        {
            Journal = world,
            Inventory = world,
            Execute = world,
            Probe = world,
            Approval = world,
            Secrets = world,
            Notify = new Notifier(),
            Scheduler = world,
            Filesystem = true,
            StdinPreamble = true
        };

        Serve.Write(new JsonObject { ["register"] = Serve.Registration(name, hooks) });
        if (Console.In.ReadLine() is null)
        {
            return 1;
        }

        string? line;
        while ((line = Console.In.ReadLine()) is not null)
        {
            line = line.Trim();
            if (line.Length == 0)
            {
                continue;
            }

            JsonObject? frame;
            try
            {
                frame = JsonNode.Parse(line) as JsonObject;
            }
            catch (JsonException)
            {
                continue;
            }

            if (frame is null || frame["kind"] is null)
            {
                continue;
            }

            var probe = frame["probe"]?.GetValue<string>();
            if (frame["kind"]?.GetValue<string>() == "probe" && probe is not null &&
                Provocations.Contains(probe))
            {
                var id = frame["id"]?.DeepClone();
                // Deliberately malformed, and deliberately not through the SDK.
                switch (probe)
                {
                    case "conform-missing-field":
                        Serve.Write(new JsonObject { ["id"] = id, ["ok"] = true });
                        break;
                    case "conform-no-ok":
                        Serve.Write(new JsonObject { ["id"] = id });
                        break;
                    // conform-silent: say nothing at all.
                }

                continue;
            }

            Serve.Write(hooks.Answer(frame));
        }

        return 0;
    }
}
