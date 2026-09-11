using System.Text.Json.Nodes;
using Xunit;

namespace Rue.Hook.Tests;

/// The loop: one reply per request line, nothing for anything else, and a budget.
public class ServeTests
{
    private static List<JsonObject> Loop(string input, Hooks hooks, TimeSpan? budget)
    {
        var output = new StringWriter();
        Serve.Loop(new StringReader(input), output, hooks, budget);
        return output.ToString()
            .Split('\n', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .Select(line => (JsonObject)JsonNode.Parse(line)!)
            .ToList();
    }

    private static Hooks Probe(int sleepMs) => new()
    {
        Probe = new FnProbe((_, _) =>
        {
            Thread.Sleep(sleepMs);
            return Hooks.Observation("yes", "ok");
        })
    };

    private const string Observe = "\"kind\":\"probe\",\"op\":\"observe\",\"host\":\"h\",\"probe\":\"up\"";

    [Fact]
    public void OnlyRequestsAreAnsweredAndEachOnce()
    {
        var input = string.Join("\n",
            "",
            "not json at all",
            "[\"an\",\"array\"]",
            "{\"event\":{\"plan\":\"p\"}}",
            "{\"id\":1}",
            $"{{\"id\":2,{Observe}}}",
            new string('[', 50_000),
            $"{{\"id\":3,{Observe}}}");
        var replies = Loop(input, Probe(0), null);
        Assert.Equal(2, replies.Count);
        Assert.Equal(2, replies[0]["id"]!.GetValue<long>());
        // A pathological line did not end the loop.
        Assert.Equal(3, replies[1]["id"]!.GetValue<long>());
    }

    [Fact]
    public void AHandlerOverItsBudgetAnswersNoAndSaysWhy()
    {
        var line = $"{{\"id\":9,{Observe}}}\n";
        var slow = Loop(line, Probe(150), TimeSpan.FromMilliseconds(20)).Single();
        Assert.Equal(9, slow["id"]!.GetValue<long>());
        Assert.False(slow["ok"]!.GetValue<bool>());
        Assert.Contains("budget", slow["error"]!.GetValue<string>());
        var quick = Loop(line, Probe(0), TimeSpan.FromSeconds(5)).Single();
        Assert.True(quick["ok"]!.GetValue<bool>());
    }

    [Fact]
    public void AReplyThatCannotBeWrittenBecomesARefusalNotADeadHook()
    {
        var h = new Hooks
        {
            Probe = new FnProbe((_, _) => new Dictionary<string, object?> { ["text"] = "", ["tri"] = double.NaN })
        };
        var input = $"{{\"id\":5,{Observe}}}\n{{\"id\":6,{Observe}}}\n";
        var replies = Loop(input, h, null);
        Assert.Equal(2, replies.Count);
        Assert.False(replies[0]["ok"]!.GetValue<bool>());
        Assert.Equal(6, replies[1]["id"]!.GetValue<long>());
    }
}
