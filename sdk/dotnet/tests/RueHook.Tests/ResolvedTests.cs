using System.Text.Json;
using System.Text.Json.Nodes;
using Xunit;

namespace Rue.Hook.Tests;

/// 7.11: an SDK exposes execute.run's secrets to the run handler "without
/// ever placing them on a command line". The value has to be reachable --
/// conformance proves that -- and it must not be reachable by accident: a
/// body concatenated into a command, logged, or written back out must not
/// carry the secret's text. This SDK handed handlers the wire objects, whose
/// ToString printed it.
public class ResolvedTests
{
    private const string Pw = "correct-horse-battery";

    [Fact]
    public void ASecretIsRedactedWhereverItIsFormatted()
    {
        var r = new Resolved(Pw, true);
        Assert.DoesNotContain(Pw, r.ToString());
        Assert.DoesNotContain(Pw, $"cmd {r}");
        Assert.DoesNotContain(Pw, string.Format("{0}", r));
        Assert.DoesNotContain(Pw, JsonSerializer.Serialize(new Dictionary<string, object> { ["k"] = r }));
        var node = JsonValue.Create(r)!;
        Assert.DoesNotContain(Pw, node.ToJsonString());
        Assert.DoesNotContain(Pw, node.ToString());
        // Expose is the one way to read it.
        Assert.Equal(Pw, Hooks.Expose(node));
    }

    [Fact]
    public void AValueThatIsNotSecretFormatsAsItsText()
    {
        var r = new Resolved("plain", false);
        Assert.Equal("plain", r.ToString());
        Assert.Equal("plain", Hooks.Expose(JsonValue.Create(r)));
    }

    [Fact]
    public void ARunHandlersBodyCarriesItsSecretsAsResolvedValues()
    {
        var seen = new List<JsonNode?>();
        var hooks = new Hooks
        {
            Execute = new FnExecute((_, _, body) =>
            {
                seen.AddRange(body);
                return Hooks.Output("", new Dictionary<string, object?>());
            })
        };
        var request = Frames.Request("execute", "run",
            ",\"host\":\"h\",\"instance\":\"i\",\"body\":[{\"run\":{\"cmd\":{\"text\":\"deploy\",\"secret\":false}," +
            $"\"env\":[[\"PW\",{{\"text\":\"{Pw}\",\"secret\":true}}]]}}}}]");
        var reply = hooks.Answer(request);
        Assert.True(reply["ok"]!.GetValue<bool>(), reply.ToJsonString());

        // The whole body, formatted the careless way, carries no secret...
        var careless = string.Join(" ", seen.Select(n => n?.ToString()));
        Assert.DoesNotContain(Pw, careless);
        Assert.DoesNotContain(Pw, string.Join(" ", seen.Select(n => n?.ToJsonString())));
        // ...and the value is still there for the handler that asks for it.
        var run = seen[0]!["run"]!;
        var secret = run["env"]![0]![1];
        Assert.True(secret is JsonValue v && v.TryGetValue<Resolved>(out _), secret?.GetType().Name);
        Assert.Equal(Pw, Hooks.Expose(secret));
        Assert.Equal("deploy", Hooks.Expose(run["cmd"]));
    }
}
