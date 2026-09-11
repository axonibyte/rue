using System.Text.Json.Nodes;
using Xunit;

namespace Rue.Hook.Tests;

/// Dispatch: every request gets a reply carrying its id, and every refusal says why.
public class HooksTests
{
    [Fact]
    public void AnOpTheProtocolDoesNotHaveIsRefusedByName()
    {
        var r = new Hooks().Answer(Frames.Request("execute", "teleport"));
        Assert.Equal(42, r["id"]!.GetValue<long>());
        Assert.False(r["ok"]!.GetValue<bool>());
        Assert.Contains("execute.teleport", r["error"]!.GetValue<string>());
        Assert.False(new Hooks().Answer(Frames.Request("weather", "report"))["ok"]!.GetValue<bool>());
    }

    [Fact]
    public void AnOpOfAKindThisHookDoesNotServeIsRefusedNotSilent()
    {
        var r = new Hooks().Answer(Frames.Request("journal", "append", ",\"entry\":{}"));
        Assert.Equal(42, r["id"]!.GetValue<long>());
        Assert.False(r["ok"]!.GetValue<bool>());
        Assert.Contains("does not serve journal.append", r["error"]!.GetValue<string>());
    }

    [Fact]
    public void AHandlersRefusalAndAHandlersFaultBothBecomeReasons()
    {
        var h = new Hooks
        {
            Notify = new FnNotify((level, _, _) =>
            {
                if (level == "warn")
                {
                    throw new Refusal("paging is off tonight");
                }

                throw new InvalidOperationException("socket closed");
            })
        };
        var refused = h.Answer(Frames.Request("notify", "deliver", ",\"level\":\"warn\""));
        Assert.Equal("paging is off tonight", refused["error"]!.GetValue<string>());
        var fault = h.Answer(Frames.Request("notify", "deliver", ",\"level\":\"err\""));
        Assert.False(fault["ok"]!.GetValue<bool>());
        Assert.Contains("InvalidOperationException: socket closed", fault["error"]!.GetValue<string>());
    }

    /// The id node was attached to the half-built reply and then handed to
    /// the refusal too; a node has one parent, so this threw out of Answer
    /// instead of refusing, and ended the hook's loop.
    [Fact]
    public void AReplyMissingARequiredFieldIsRefusedHereNotSentForR0303()
    {
        var h = new Hooks
        {
            Approval = new FnVerify(() => new Dictionary<string, object?> { ["reason"] = "forgot the verdict" })
        };
        var r = h.Answer(Frames.Request("approval", "verify"));
        Assert.Equal(42, r["id"]!.GetValue<long>());
        Assert.False(r["ok"]!.GetValue<bool>());
        Assert.Contains("without verified", r["error"]!.GetValue<string>());
    }

    /// The reply's fields were serialized outside the handler's try, so a
    /// value the serializer refuses threw out of Answer.
    [Fact]
    public void AReplyTheSerializerRefusesIsARefusalNotAFault()
    {
        var h = new Hooks
        {
            Probe = new FnProbe((_, _) => new Dictionary<string, object?> { ["text"] = "", ["tri"] = double.NaN })
        };
        var r = h.Answer(Frames.Request("probe", "observe", ",\"host\":\"h\",\"probe\":\"x\""));
        Assert.Equal(42, r["id"]!.GetValue<long>());
        Assert.False(r["ok"]!.GetValue<bool>());
    }

    [Fact]
    public void ReadFactsNoSuchFileIsAnAnswerWithNoContent()
    {
        var h = new Hooks
        {
            Execute = new FnExecute(
                (_, _, _) => Hooks.Output("", new Dictionary<string, object?>()),
                (_, shape) => shape.EndsWith("present") ? "" : null)
        };
        var absent = h.Answer(Frames.Request("execute", "read_fact", ",\"shape\":\"file:/absent\""));
        Assert.True(absent["ok"]!.GetValue<bool>());
        Assert.False(absent.ContainsKey("content"));
        var empty = h.Answer(Frames.Request("execute", "read_fact", ",\"shape\":\"file:/present\""));
        // An empty file is not an absent one.
        Assert.Equal("", empty["content"]!.GetValue<string>());
    }

    [Fact]
    public void TheRegistrationNamesOnlyTheKindsSupplied()
    {
        var h = new Hooks
        {
            Probe = new FnProbe((_, _) => Hooks.Observation("yes")),
            Notify = new FnNotify((_, _, _) => { })
        };
        var reg = Serve.Registration("x", h);
        Assert.Equal("[\"probe\",\"notify\"]", reg["kinds"]!.ToJsonString());
        Assert.Equal(Op.HookProtocol, reg["protocol"]!.GetValue<int>());
    }
}
