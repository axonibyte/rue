defmodule RueHook.Resolved do
  @moduledoc """
  A value of an `execute.run` body after resolution: its text, and whether
  it is a secret.

  7.11 has an SDK expose a run's secrets to the handler "without ever
  placing them on a command line". The text is reachable through
  `RueHook.expose/1`, named so that reading a secret is a visible act; but
  inspecting, interpolating or encoding the value -- which is how a secret
  reaches a command line or a log by accident -- gives `"<secret>"` and
  never the text. The Rust, Python and Java SDKs carry the same type.
  """

  defstruct text: "", secret: false

  @type t :: %__MODULE__{text: String.t(), secret: boolean()}

  @doc "What a secret reads as wherever it is formatted."
  def redacted, do: "<secret>"

  @doc false
  def of(%{"text" => text} = wire),
    do: %__MODULE__{text: to_string(text), secret: Map.get(wire, "secret") == true}

  @doc """
  A body as a run handler receives it: the same maps and lists as the
  wire, with every resolved value -- a primitive's field, or the value of
  an `env` pair -- a `RueHook.Resolved`.
  """
  def body(wire) when is_list(wire), do: Enum.map(wire, &prim/1)
  def body(other), do: other

  defp prim(%{} = p), do: Map.new(p, fn {name, fields} -> {name, fields(fields)} end)
  defp prim(other), do: other

  defp fields(%{} = f), do: Map.new(f, fn {k, v} -> {k, field(k, v)} end)
  defp fields(other), do: other

  defp field(_k, %{"text" => _} = v), do: of(v)

  defp field("env", pairs) when is_list(pairs) do
    Enum.map(pairs, fn
      [k, %{"text" => _} = v] -> [k, of(v)]
      other -> other
    end)
  end

  defp field(_k, v), do: v
end

defimpl Inspect, for: RueHook.Resolved do
  def inspect(%{secret: true}, _opts), do: "#RueHook.Resolved<secret>"
  def inspect(%{text: text}, _opts), do: "#RueHook.Resolved<" <> Kernel.inspect(text) <> ">"
end

defimpl String.Chars, for: RueHook.Resolved do
  def to_string(%{secret: true}), do: RueHook.Resolved.redacted()
  def to_string(%{text: text}), do: text
end

defimpl JSON.Encoder, for: RueHook.Resolved do
  # A Resolved a handler put in its reply is written as it formats: a
  # secret as its redaction, never its text.
  def encode(r, encoder), do: encoder.(to_string(r), encoder)
end
