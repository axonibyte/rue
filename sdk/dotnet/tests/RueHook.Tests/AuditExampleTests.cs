using System.Text.Json.Nodes;
using Rue.Hook.Example;
using Xunit;

namespace Rue.Hook.Tests;

/// examples/AuditHook, the quick start of docs/README.md, does what the page
/// says it does.
public sealed class AuditExampleTests : IDisposable
{
    private readonly string _dir =
        Path.Combine(Path.GetTempPath(), $"rue-audit-example-{Guid.NewGuid():N}");

    public AuditExampleTests() => Directory.CreateDirectory(_dir);

    public void Dispose() => Directory.Delete(_dir, recursive: true);

    [Fact]
    public void ItRegistersForJournalAndNotify()
    {
        Assert.Equal(new[] { "journal", "notify" }, AuditHook.Build(Path.Combine(_dir, "audit.ndjson")).Kinds());
    }

    [Fact]
    public void EachEntryIsAppendedAsOneLine()
    {
        var log = Path.Combine(_dir, "audit.ndjson");
        var hooks = AuditHook.Build(log);
        foreach (var seq in new[] { 1, 2 })
        {
            var reply = hooks.Answer(Frames.Request("journal", "append", $",\"entry\":{{\"seq\":{seq}}}"));
            Assert.Equal("{\"id\":42,\"ok\":true}", reply.ToJsonString());
        }

        var seqs = File.ReadAllLines(log).Select(l => JsonNode.Parse(l)!["seq"]!.GetValue<int>());
        Assert.Equal(new[] { 1, 2 }, seqs);
    }

    [Fact]
    public void AnEntryItCannotRecordIsRefusedWithTheReason()
    {
        var hooks = AuditHook.Build(Path.Combine(_dir, "no-such-dir", "audit.ndjson"));
        var reply = hooks.Answer(Frames.Request("journal", "append", ",\"entry\":{\"seq\":1}"));
        Assert.False(reply["ok"]!.GetValue<bool>());
        Assert.Contains("is not writable", reply["error"]!.GetValue<string>());
    }

    [Fact]
    public void ANotificationGoesToStderrAndIsAcknowledged()
    {
        var was = Console.Error;
        var err = new StringWriter();
        Console.SetError(err);
        JsonObject reply;
        try
        {
            reply = AuditHook.Build(Path.Combine(_dir, "audit.ndjson")).Answer(Frames.Request("notify", "deliver",
                ",\"level\":\"warn\",\"subject\":\"plan held\",\"body\":\"waiting for approval\""));
        }
        finally
        {
            Console.SetError(was);
        }

        Assert.True(reply["ok"]!.GetValue<bool>());
        Assert.Equal("[warn] plan held: waiting for approval" + Environment.NewLine, err.ToString());
    }
}
