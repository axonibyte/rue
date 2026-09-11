defmodule RueHook.ClientTest do
  use ExUnit.Case, async: true

  alias RueHook.{Client, Hooks}

  defmodule P do
    def observe(_h, _p), do: {:ok, RueHook.observation(:yes, "ok")}
  end

  test "a request of a kind no hook on the connection serves is refused, not a crash" do
    # The hook was looked up with String.to_existing_atom(kind), which
    # raises on a kind that was never an atom -- ending the GenServer and
    # with it the embedding host's connection.
    hooks = %{"p" => %Hooks{probe: P}}
    assert Client.hook_for(hooks, "probe") == %Hooks{probe: P}
    assert Client.hook_for(hooks, "journal") == nil
    assert Client.hook_for(hooks, "no-such-kind-#{System.unique_integer()}") == nil
  end
end
