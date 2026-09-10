using System.Text.Json;
using System.Text.Json.Nodes;

namespace Rue.Hook;

/// <summary>Why a hook will not answer: the text the engine journals and
/// the operator reads. Throwing it is a hook saying no, on the record, and
/// is not a fault.</summary>
public sealed class Refusal : Exception
{
    public Refusal(string why) : base(why) { }

    public static Refusal Unserved(string kind, string op) =>
        new($"this hook does not serve {kind}.{op}");
}

public interface IJournal
{
    void Append(JsonNode entry);
}

public interface IInventory
{
    IEnumerable<object> List();
}

/// The instance-directory ops below <c>Run</c> are required only of a hook
/// that registers <c>filesystem</c> (7.7).
public interface IExecute
{
    object Run(string host, string instance, IReadOnlyList<JsonNode?> body);

    /// The file's text, or null for no such file -- an answer, not a refusal.
    string? ReadFact(string host, string shape) => throw Refusal.Unserved("execute", "read_fact");

    object BootstrapState(string host) => throw Refusal.Unserved("execute", "bootstrap_state");

    /// The one op the engine reads a refusal of as "no skew probe is
    /// possible here" rather than as a fault.
    long Clock(string host) => throw Refusal.Unserved("execute", "clock");

    void InstanceDirCreate(string host, string instance) =>
        throw Refusal.Unserved("execute", "instance_dir_create");

    void InstanceDirRemove(string host, string instance) =>
        throw Refusal.Unserved("execute", "instance_dir_remove");

    IEnumerable<object> InstanceDirList(string host) =>
        throw Refusal.Unserved("execute", "instance_dir_list");

    void PutFile(string host, string instance, string rel, string content, long mode) =>
        throw Refusal.Unserved("execute", "put_file");

    void ReplaceFile(string host, string instance, string rel, string content) =>
        throw Refusal.Unserved("execute", "replace_file");

    string GetFile(string host, string instance, string rel) =>
        throw Refusal.Unserved("execute", "get_file");

    void RemoveFile(string host, string instance, string rel) =>
        throw Refusal.Unserved("execute", "remove_file");

    void HostLock(string host) => throw Refusal.Unserved("execute", "host_lock");
}

public interface IProbe
{
    object Observe(string host, string probe);
}

public interface IApproval
{
    IEnumerable<object> Authenticators();

    string Challenge(string instance, string digest, JsonNode? scope, JsonNode? context);

    object Verify(string instance, string digest, JsonNode? scope, string authenticator, string proof);
}

public interface ISecrets
{
    string Resolve(string reference) => throw Refusal.Unserved("secrets", "resolve");

    object Deliver(string instance, string label, string value) =>
        throw Refusal.Unserved("secrets", "deliver");
}

public interface INotify
{
    void Deliver(string level, string subject, string body);
}

public interface IScheduler
{
    void Install(string host, string artifact);

    void Arm(string host, string artifact, JsonNode? deadline);

    void Rearm(string host, string artifact, JsonNode? deadline);

    void Disarm(string host, string artifact);

    /// true, false, or the string "unknown". Never guess: the engine reads
    /// false as "install it again", which is destructive when it is wrong.
    object Present(string host, string artifact);
}

/// <summary>
/// What this hook serves, and the dispatch from a request frame to a handler.
///
/// The registration frame is built from the kinds actually supplied, so a
/// hook cannot register for one it does not serve -- the failure that
/// produces is a plan binding to it and refusing at its first step, a long
/// way from where the mistake was made.
/// </summary>
public sealed class Hooks
{
    public IJournal? Journal { get; set; }
    public IInventory? Inventory { get; set; }
    public IExecute? Execute { get; set; }
    public IProbe? Probe { get; set; }
    public IApproval? Approval { get; set; }
    public ISecrets? Secrets { get; set; }
    public INotify? Notify { get; set; }
    public IScheduler? Scheduler { get; set; }

    /// This hook serves the instance-directory ops (7.7).
    public bool Filesystem { get; set; }

    public bool StdinPreamble { get; set; }

    public List<string> Kinds()
    {
        var k = new List<string>();
        if (Journal is not null) k.Add("journal");
        if (Inventory is not null) k.Add("inventory");
        if (Execute is not null) k.Add("execute");
        if (Probe is not null) k.Add("probe");
        if (Approval is not null) k.Add("approval");
        if (Secrets is not null) k.Add("secrets");
        if (Notify is not null) k.Add("notify");
        if (Scheduler is not null) k.Add("scheduler");
        return k;
    }

    // --- little constructors, so a handler never hand-builds a wire map ---

    public static object Observation(string tri, string text = "") =>
        new Dictionary<string, object?> { ["text"] = text, ["tri"] = tri };

    public static object Verdict(bool verified, string reason) =>
        new Dictionary<string, object?> { ["verified"] = verified, ["reason"] = reason };

    public static object Delivery(bool accepted, string receipt) =>
        new Dictionary<string, object?> { ["accepted"] = accepted, ["receipt"] = receipt };

    public static object Authenticator(string id, bool human) =>
        new Dictionary<string, object?> { ["id"] = id, ["human"] = human };

    public static object Output(string stdout, IDictionary<string, object?> outputs) =>
        new Dictionary<string, object?> { ["stdout"] = stdout, ["outputs"] = outputs };

    /// The text of a resolved value. Named rather than reached through the
    /// node so that reading a secret is a visible act in the code that does it.
    public static string Expose(JsonNode? resolved) =>
        resolved?["text"]?.GetValue<string>() ?? string.Empty;

    /// <summary>
    /// Answer one request frame. The reply always carries the request's id,
    /// including for an op with no handler: a request left unanswered is
    /// Silent, and the engine can only report that as "the step did not
    /// happen".
    /// </summary>
    public JsonObject Answer(JsonObject request)
    {
        var id = request["id"]?.DeepClone();
        var kind = Str(request, "kind");
        var opName = Str(request, "op");
        var row = Op.Find(kind, opName);
        if (row is null)
        {
            return RefusalFrame(id, $"{kind}.{opName} is not an op of this protocol");
        }

        Dictionary<string, object?> fields;
        try
        {
            fields = Dispatch(row, request);
        }
        catch (Refusal why)
        {
            return RefusalFrame(id, why.Message);
        }
        catch (Exception e)
        {
            return RefusalFrame(id, $"{e.GetType().Name}: {e.Message}");
        }

        var reply = new JsonObject { ["id"] = id, ["ok"] = true };
        foreach (var (k, v) in fields)
        {
            reply[k] = JsonSerializer.SerializeToNode(v);
        }

        foreach (var f in row.RequiredReply)
        {
            if (reply[f] is null)
            {
                // Replies are built from the op's row, so this is a bug here
                // and not an R0303 for the far end to puzzle over.
                return RefusalFrame(id, $"the SDK built a {kind}.{opName} reply without {f}");
            }
        }

        return reply;
    }

    private static JsonObject RefusalFrame(JsonNode? id, string why) =>
        new() { ["id"] = id, ["ok"] = false, ["error"] = why };

    private static string Str(JsonObject o, string k) =>
        o[k] is { } n && n.GetValueKind() == JsonValueKind.String ? n.GetValue<string>() : "";

    private static Dictionary<string, object?> One(string k, object? v) => new() { [k] = v };

    private static Dictionary<string, object?> None() => new();

    private Dictionary<string, object?> Dispatch(Op row, JsonObject r)
    {
        var host = Str(r, "host");
        var inst = Str(r, "instance");
        switch (row.Kind)
        {
            case "journal":
                {
                    var h = Journal ?? throw Refusal.Unserved(row.Kind, row.OpName);
                    var entry = r["entry"] ?? throw new Refusal("journal.append without an entry");
                    h.Append(entry);
                    return None();
                }
            case "inventory":
                {
                    var h = Inventory ?? throw Refusal.Unserved(row.Kind, row.OpName);
                    return One("hosts", h.List());
                }
            case "probe":
                {
                    var h = Probe ?? throw Refusal.Unserved(row.Kind, row.OpName);
                    return One("fact", h.Observe(host, Str(r, "probe")));
                }
            case "notify":
                {
                    var h = Notify ?? throw Refusal.Unserved(row.Kind, row.OpName);
                    h.Deliver(Str(r, "level"), Str(r, "subject"), Str(r, "body"));
                    return None();
                }
            case "approval":
                {
                    var h = Approval ?? throw Refusal.Unserved(row.Kind, row.OpName);
                    return row.OpName switch
                    {
                        "authenticators" => One("authenticators", h.Authenticators()),
                        "challenge" => One("challenge",
                            h.Challenge(inst, Str(r, "digest"), r["scope"], r["context"])),
                        _ => Flatten(h.Verify(inst, Str(r, "digest"), r["scope"],
                            Str(r, "authenticator"), Str(r, "proof")))
                    };
                }
            case "secrets":
                {
                    var h = Secrets ?? throw Refusal.Unserved(row.Kind, row.OpName);
                    return row.OpName == "resolve"
                        ? One("value", h.Resolve(Str(r, "ref")))
                        : Flatten(h.Deliver(inst, Str(r, "label"), Str(r, "value")));
                }
            case "scheduler":
                {
                    var h = Scheduler ?? throw Refusal.Unserved(row.Kind, row.OpName);
                    var artifact = Str(r, "artifact");
                    switch (row.OpName)
                    {
                        case "install": h.Install(host, artifact); return None();
                        case "arm": h.Arm(host, artifact, r["deadline"]); return None();
                        case "rearm": h.Rearm(host, artifact, r["deadline"]); return None();
                        case "disarm": h.Disarm(host, artifact); return None();
                        default: return One("present", h.Present(host, artifact));
                    }
                }
            default:
                {
                    var h = Execute ?? throw Refusal.Unserved(row.Kind, row.OpName);
                    switch (row.OpName)
                    {
                        case "run":
                            {
                                var body = r["body"] as JsonArray ?? new JsonArray();
                                return One("output", h.Run(host, inst, body.ToList()));
                            }
                        case "read_fact":
                            {
                                var c = h.ReadFact(host, Str(r, "shape"));
                                // null is "no such file", which is an answer;
                                // an empty string would be a file that exists
                                // and is empty.
                                return c is null ? None() : One("content", c);
                            }
                        case "bootstrap_state": return One("state", h.BootstrapState(host));
                        case "clock": return One("epoch_s", h.Clock(host));
                        case "instance_dir_create": h.InstanceDirCreate(host, inst); return None();
                        case "instance_dir_remove": h.InstanceDirRemove(host, inst); return None();
                        case "instance_dir_list": return One("dirs", h.InstanceDirList(host));
                        case "put_file":
                            h.PutFile(host, inst, Str(r, "rel"), Str(r, "content"),
                                r["mode"]?.GetValue<long>() ?? 0);
                            return None();
                        case "replace_file":
                            h.ReplaceFile(host, inst, Str(r, "rel"), Str(r, "content"));
                            return None();
                        case "get_file": return One("content", h.GetFile(host, inst, Str(r, "rel")));
                        case "remove_file": h.RemoveFile(host, inst, Str(r, "rel")); return None();
                        default: h.HostLock(host); return None();
                    }
                }
        }
    }

    /// A handler that returns a whole reply body (a verdict, a delivery)
    /// hands back its fields rather than one named value.
    private static Dictionary<string, object?> Flatten(object v)
    {
        if (v is IDictionary<string, object?> d)
        {
            return new Dictionary<string, object?>(d);
        }
        throw new Refusal("the handler returned something that is not a set of reply fields");
    }
}
