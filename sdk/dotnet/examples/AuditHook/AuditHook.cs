using System.Text.Json.Nodes;
using Rue.Hook;

namespace Rue.Hook.Example;

/// <summary>
/// An audit hook: a journal sink that keeps every entry, and a notifier.
///
/// Bind it in a site with <c>journal to: local(), hook(:audit)</c> and
/// <c>notify via: hook(:audit)</c>, and have rued spawn it:
///
///     rued run --spawn audit="dotnet /opt/rue/AuditHook.dll" ...
///
/// Each journal entry is appended to $RUE_AUDIT_LOG (default audit.ndjson)
/// as one line of JSON. A sink that cannot record an entry must say so: the
/// engine then refuses to proceed (R0304) rather than run a step nobody
/// recorded. Notifications go to stderr, because stdout carries the protocol.
/// </summary>
public static class AuditHook
{
    private sealed class AuditLog(string path) : IJournal
    {
        public void Append(JsonNode entry)
        {
            try
            {
                File.AppendAllText(path, entry.ToJsonString() + "\n");
            }
            catch (Exception e) when (e is IOException or UnauthorizedAccessException)
            {
                throw new Refusal($"the audit log {path} is not writable: {e.Message}");
            }
        }
    }

    private sealed class Stderr : INotify
    {
        public void Deliver(string level, string subject, string body) =>
            Console.Error.WriteLine($"[{level}] {subject}: {body}");
    }

    public static Hooks Build(string path) => new() { Journal = new AuditLog(path), Notify = new Stderr() };

    public static int Main() =>
        Serve.Stdio("audit", Build(Environment.GetEnvironmentVariable("RUE_AUDIT_LOG") ?? "audit.ndjson"));
}
