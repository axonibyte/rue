defmodule RueHook do
  @moduledoc """
  The Elixir embedding SDK for rue's hook protocol (docs/ROADMAP.md 7.11).

  No dependencies: Elixir 1.18 ships `JSON` in the standard library, and a
  hook answers lines of JSON, so a host application is asked to take
  nothing it did not already have.

      defmodule Fence do
        def observe(_host, probe), do: {:ok, RueHook.observation(:yes, probe)}
      end

      RueHook.Serve.stdio("fence", %RueHook.Hooks{probe: Fence})

  What the SDK is for, past the framing: a reply is built from the op's own
  row in `RueHook.Proto`, so a hook written on it cannot answer `ok: true`
  without a field the op requires. That is R0303 at the engine, and R0303
  refuses the step; the SDK's job is to put it out of reach.

  Secrets cross this boundary in exactly four messages (7.5). An
  `execute.run` hands its handler the resolved body with its secrets
  intact; `expose/1` is how a value is read, named so that using one is a
  visible act in the code that does it.
  """

  @doc "A probe's answer, as the wire carries it."
  def observation(tri, text \\ "") when tri in [:yes, :no, :unknown],
    do: %{"text" => text, "tri" => Atom.to_string(tri)}

  @doc """
  The text of a resolved value.

  Named rather than reached through the map so that reading a secret is a
  visible act. `inspect/1` on a body shows a secret's text like any other
  map value, so do not log one.
  """
  def expose(%{"text" => text}), do: text
  def expose(other) when is_binary(other), do: other

  @doc "True when any value of a resolved primitive is a secret."
  def carries_secret?(%{} = prim) do
    prim
    |> Map.values()
    |> Enum.any?(fn
      %{"secret" => true} -> true
      list when is_list(list) -> Enum.any?(list, &match?([_, %{"secret" => true}], &1))
      _ -> false
    end)
  end
end
