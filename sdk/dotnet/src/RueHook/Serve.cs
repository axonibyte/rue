using System.Text.Json;
using System.Text.Json.Nodes;

namespace Rue.Hook;

/// <summary>
/// The way <c>rued</c> reaches a hook it spawned (docs/hook-protocol.md,
/// "A hook over stdio").
///
/// Send the registration frame, read the acknowledgement, then answer one
/// request per line until stdin closes. Event frames from a subscription
/// are skipped: a hook that is not also an operator has nothing to do with
/// them.
/// </summary>
public static class Serve
{
    public static JsonObject Registration(string name, Hooks hooks)
    {
        var kinds = new JsonArray();
        foreach (var k in hooks.Kinds())
        {
            kinds.Add(k);
        }

        return new JsonObject
        {
            ["name"] = name,
            ["kinds"] = kinds,
            ["protocol"] = Op.HookProtocol,
            ["filesystem"] = hooks.Filesystem,
            ["stdin_preamble"] = hooks.StdinPreamble
        };
    }

    /// <summary>
    /// Serve as a child the daemon spawned (<c>rued run --spawn</c>).
    ///
    /// The registration frame is the first line of stdout, before anything
    /// else, so keep your own logging on stderr.
    /// </summary>
    public static int Stdio(string name, Hooks hooks)
    {
        Write(new JsonObject { ["register"] = Registration(name, hooks) });
        var ack = Console.In.ReadLine();
        if (ack is null || !Acknowledged(ack))
        {
            Console.Error.WriteLine($"rue-hook: registration was not acknowledged: {ack}");
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

            Write(hooks.Answer(frame));
        }

        return 0;
    }

    /// One line, flushed: an unflushed reply is a silence, and a silence is
    /// a refusal of the step with nothing to say about why.
    public static void Write(JsonNode frame)
    {
        Console.Out.WriteLine(frame.ToJsonString());
        Console.Out.Flush();
    }

    private static bool Acknowledged(string line)
    {
        try
        {
            return JsonNode.Parse(line)?["register"]?["ok"]?.GetValue<bool>() == true;
        }
        catch (JsonException)
        {
            return false;
        }
    }
}
