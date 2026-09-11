defmodule RueHook.ServeTest do
  # The loop: one reply per request line, nothing for anything else, a
  # budget, and no line or reply that ends the hook.
  use ExUnit.Case, async: true

  alias RueHook.{Hooks, Serve}

  defmodule Slow do
    def observe(_host, "slow"), do: (Process.sleep(150); {:ok, RueHook.observation(:yes, "ok")})
    def observe(_host, "tuple"), do: {:ok, %{"text" => {:not, :json}, "tri" => "yes"}}
    def observe(_host, _), do: {:ok, RueHook.observation(:yes, "ok")}
  end

  defp req(id, probe \\ "up"),
    do: JSON.encode!(%{"id" => id, "kind" => "probe", "op" => "observe", "host" => "h", "probe" => probe})

  defp serve(lines, budget_ms \\ nil) do
    {:ok, input} = StringIO.open(Enum.join(lines, "\n") <> "\n")
    {:ok, output} = StringIO.open("")
    :ok = Serve.pump(input, output, %Hooks{probe: Slow}, budget_ms)
    {_, written} = StringIO.contents(output)
    written |> String.split("\n", trim: true) |> Enum.map(&JSON.decode!/1)
  end

  test "only requests are answered, and each once" do
    replies = serve(["", "not json", ~s(["an","array"]), ~s({"event":{"plan":"p"}}), ~s({"id":1}), req(2), req(3)])
    assert Enum.map(replies, & &1["id"]) == [2, 3]
  end

  test "a handler over its budget answers no and says why" do
    [slow] = serve([req(9, "slow")], 20)
    assert {slow["id"], slow["ok"]} == {9, false}
    assert slow["error"] =~ "budget"
    [quick] = serve([req(9)], 5_000)
    assert quick["ok"] == true
  end

  test "a reply that cannot be written becomes a refusal, not a dead hook" do
    replies = serve([req(5, "tuple"), req(6)])
    assert Enum.map(replies, & &1["id"]) == [5, 6]
    assert hd(replies)["ok"] == false
  end
end
