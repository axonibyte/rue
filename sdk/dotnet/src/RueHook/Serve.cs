using System.Diagnostics;
using System.Text;
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
    ///
    /// With a <paramref name="budget"/>, a handler that overruns it answers
    /// <c>ok: false</c> naming the overrun. The engine's deadline
    /// (<c>rued run --hook-deadline</c>) is not on the wire, so an SDK cannot
    /// see it; what it can do is keep its own slowness from arriving as a
    /// silence, because a refusal with a reason is worth more to the operator
    /// than a timeout. Null leaves a slow handler to the engine's deadline.
    /// The Rust, Python, Java and Elixir SDKs do the same.
    /// </summary>
    public static int Stdio(string name, Hooks hooks, TimeSpan? budget = null)
    {
        // UTF-8 both ways, whatever the locale says: the engine writes UTF-8,
        // and a console decoding it as ASCII would hand handlers mangled text.
        var utf8 = new UTF8Encoding(false);
        var input = new StreamReader(Console.OpenStandardInput(), utf8);
        var output = new StreamWriter(Console.OpenStandardOutput(), utf8) { AutoFlush = true };
        Write(output, new JsonObject { ["register"] = Registration(name, hooks) });
        var ack = input.ReadLine();
        if (ack is null || !Acknowledged(ack))
        {
            Console.Error.WriteLine($"rue-hook: registration was not acknowledged: {ack}");
            return 1;
        }

        Loop(input, output, hooks, budget);
        return 0;
    }

    /// <summary>
    /// Answer one request per line of <paramref name="input"/> until it ends:
    /// the loop <see cref="Stdio"/> runs after registering. Blank lines, lines
    /// that are not a JSON object, event frames and frames with no
    /// <c>kind</c> get no reply; every request gets exactly one.
    /// </summary>
    public static void Loop(TextReader input, TextWriter output, Hooks hooks, TimeSpan? budget)
    {
        string? line;
        while ((line = input.ReadLine()) is not null)
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

            if (frame is null || frame["kind"] is null || frame["event"] is not null)
            {
                continue;
            }

            var clock = Stopwatch.StartNew();
            var reply = hooks.Answer(frame);
            clock.Stop();
            if (budget is { } b && clock.Elapsed > b && reply["ok"] is JsonValue ok && ok.GetValue<bool>())
            {
                reply = Hooks.RefusalFrame(frame["id"],
                    $"the handler took {(long)clock.Elapsed.TotalMilliseconds}ms, over its " +
                    $"{(long)b.TotalMilliseconds}ms budget; answering late is worse than answering no");
            }

            Write(output, reply, frame["id"]);
        }
    }

    /// One line, flushed: an unflushed reply is a silence, and a silence is
    /// a refusal of the step with nothing to say about why.
    public static void Write(JsonNode frame) => Write(Console.Out, frame);

    private static void Write(TextWriter output, JsonNode frame, JsonNode? id = null)
    {
        string text;
        try
        {
            text = frame.ToJsonString();
        }
        catch (Exception e)
        {
            // A frame the serializer cannot write is refused by name rather
            // than ending the loop.
            text = Hooks.RefusalFrame(id, $"the reply could not be written: {e.Message}").ToJsonString();
        }

        output.WriteLine(text);
        output.Flush();
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
