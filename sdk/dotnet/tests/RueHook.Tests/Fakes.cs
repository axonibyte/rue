using System.Text.Json.Nodes;

namespace Rue.Hook.Tests;

/// Handlers from lambdas, so each test states only the behavior it is about.
internal sealed class FnProbe(Func<string, string, object> observe) : IProbe
{
    public object Observe(string host, string probe) => observe(host, probe);
}

internal sealed class FnNotify(Action<string, string, string> deliver) : INotify
{
    public void Deliver(string level, string subject, string body) => deliver(level, subject, body);
}

internal sealed class FnExecute(
    Func<string, string, IReadOnlyList<JsonNode?>, object> run,
    Func<string, string, string?>? readFact = null) : IExecute
{
    public object Run(string host, string instance, IReadOnlyList<JsonNode?> body) => run(host, instance, body);

    public string? ReadFact(string host, string shape) =>
        readFact is null ? throw Refusal.Unserved("execute", "read_fact") : readFact(host, shape);
}

internal sealed class FnVerify(Func<object> verify) : IApproval
{
    public IEnumerable<object> Authenticators() => Array.Empty<object>();

    public string Challenge(string instance, string digest, JsonNode? scope, JsonNode? context) => "c";

    public object Verify(string instance, string digest, JsonNode? scope, string authenticator, string proof) =>
        verify();
}

internal static class Frames
{
    public static JsonObject Request(string kind, string op, string extra = "") =>
        (JsonObject)JsonNode.Parse($"{{\"id\":42,\"kind\":\"{kind}\",\"op\":\"{op}\"{extra}}}")!;
}
