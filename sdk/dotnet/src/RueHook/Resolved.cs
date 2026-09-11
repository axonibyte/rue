using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization;

namespace Rue.Hook;

/// <summary>
/// A value of an <c>execute.run</c> body after resolution: its text, and
/// whether it is a secret.
///
/// 7.11 has an SDK expose a run's secrets to the handler "without ever
/// placing them on a command line". The text is reachable through
/// <see cref="Hooks.Expose"/>, named so that reading a secret is a visible
/// act; but formatting the value, or serializing it -- which is how a
/// secret reaches a command line or a log by accident -- gives
/// <c>&lt;secret&gt;</c> and never the text. A run handler finds these in
/// its body wrapped in a <c>JsonValue</c>, so its body stays a tree of
/// <c>JsonNode</c> and an existing handler keeps working. The Rust, Python,
/// Java and Elixir SDKs carry the same type.
/// </summary>
[JsonConverter(typeof(ResolvedConverter))]
public sealed record Resolved(string Text, bool Secret)
{
    /// <summary>What a secret reads as wherever it is formatted.</summary>
    public const string Redacted = "<secret>";

    public override string ToString() => Secret ? Redacted : Text;

    internal static bool IsWire(JsonNode? n) => n is JsonObject o && o.ContainsKey("text");

    internal static Resolved Of(JsonObject o)
    {
        var text = o["text"] is JsonValue t && t.TryGetValue<string>(out var s) ? s : o["text"]?.ToJsonString() ?? "";
        var secret = o["secret"] is JsonValue b && b.TryGetValue<bool>(out var isSecret) && isSecret;
        return new Resolved(text, secret);
    }

    /// A body as a run handler receives it: the same objects and arrays as
    /// the wire, with every resolved value -- a primitive's field, or the
    /// value of an <c>env</c> pair -- a <see cref="Resolved"/>.
    internal static List<JsonNode?> Body(JsonArray wire)
    {
        var body = new List<JsonNode?>(wire.Count);
        foreach (var prim in wire)
        {
            if (prim is not JsonObject p)
            {
                body.Add(prim?.DeepClone());
                continue;
            }

            var converted = new JsonObject();
            foreach (var (name, fields) in p)
            {
                if (fields is JsonObject f)
                {
                    var m = new JsonObject();
                    foreach (var (k, v) in f)
                    {
                        m[k] = Field(k, v);
                    }

                    converted[name] = m;
                }
                else
                {
                    converted[name] = fields?.DeepClone();
                }
            }

            body.Add(converted);
        }

        return body;
    }

    private static JsonNode? Field(string name, JsonNode? v)
    {
        if (IsWire(v))
        {
            return JsonValue.Create(Of((JsonObject)v!));
        }

        if (name == "env" && v is JsonArray pairs)
        {
            var env = new JsonArray();
            foreach (var pair in pairs)
            {
                if (pair is JsonArray kv && kv.Count == 2 && IsWire(kv[1]))
                {
                    env.Add(new JsonArray(kv[0]?.DeepClone(), JsonValue.Create(Of((JsonObject)kv[1]!))));
                }
                else
                {
                    env.Add(pair?.DeepClone());
                }
            }

            return env;
        }

        return v?.DeepClone();
    }
}

/// Writes a <see cref="Resolved"/> as it formats: a secret as its
/// redaction, never its text.
internal sealed class ResolvedConverter : JsonConverter<Resolved>
{
    public override Resolved Read(ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options)
    {
        var node = JsonNode.Parse(ref reader) as JsonObject
            ?? throw new JsonException("a resolved value is a JSON object with a text field");
        return Resolved.Of(node);
    }

    public override void Write(Utf8JsonWriter writer, Resolved value, JsonSerializerOptions options) =>
        writer.WriteStringValue(value.ToString());
}
