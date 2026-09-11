defmodule RueHook.HooksTest do
  # Dispatch: every request gets a reply carrying its id, every refusal a reason.
  use ExUnit.Case, async: true

  alias RueHook.Hooks

  defp req(kind, op, extra \\ %{}), do: Map.merge(%{"id" => 42, "kind" => kind, "op" => op}, extra)

  test "an op the protocol does not have is refused by name" do
    r = Hooks.answer(%Hooks{}, req("execute", "teleport"))
    assert {r["id"], r["ok"]} == {42, false}
    assert r["error"] =~ "execute.teleport"
  end

  test "an op of an unserved kind is refused, not silent" do
    r = Hooks.answer(%Hooks{}, req("journal", "append", %{"entry" => %{}}))
    assert {r["id"], r["ok"]} == {42, false}
    assert r["error"] =~ "journal.append"
  end

  defmodule Notifier do
    def deliver("warn", _s, _b), do: {:refuse, "paging is off tonight"}
    def deliver("odd", _s, _b), do: :something_else
    def deliver(_l, _s, _b), do: raise("socket closed")
  end

  test "a refusal, a fault and a malformed answer all become reasons" do
    h = %Hooks{notify: Notifier}
    assert Hooks.answer(h, req("notify", "deliver", %{"level" => "warn"}))["error"] == "paging is off tonight"
    fault = Hooks.answer(h, req("notify", "deliver", %{"level" => "err"}))
    assert fault["ok"] == false
    assert fault["error"] =~ "socket closed"
  end

  defmodule Prober do
    def observe(_host, "bare"), do: RueHook.observation(:yes, "a bare value, not {:ok, v}")
    def observe(_host, _), do: {:ok, RueHook.observation(:yes, "ok")}
  end

  test "a handler that returns neither {:ok, v} nor {:refuse, why} is refused, not a crash" do
    # The value fell through to a case with no clause for it, and the
    # CaseClauseError ended the serve loop -- or the embedding host's
    # connection.
    r = Hooks.answer(%Hooks{probe: Prober}, req("probe", "observe", %{"probe" => "bare"}))
    assert {r["id"], r["ok"]} == {42, false}
    assert r["error"] =~ "probe.observe"
  end

  defmodule Facts do
    def read_fact(_host, "file:/present"), do: {:ok, ""}
    def read_fact(_host, _), do: {:ok, nil}
  end

  test "read_fact's no-such-file is an answer with no content" do
    absent = Hooks.answer(%Hooks{execute: Facts}, req("execute", "read_fact", %{"shape" => "file:/absent"}))
    assert absent["ok"] == true
    refute Map.has_key?(absent, "content")
    present = Hooks.answer(%Hooks{execute: Facts}, req("execute", "read_fact", %{"shape" => "file:/present"}))
    assert present["content"] == ""
  end

  test "the registration names only the kinds supplied" do
    reg = RueHook.Serve.registration("x", %Hooks{probe: Prober, notify: Notifier})
    assert reg["kinds"] == ["probe", "notify"]
    assert reg["protocol"] == 1
  end
end
