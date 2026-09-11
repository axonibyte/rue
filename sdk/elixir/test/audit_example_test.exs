defmodule RueHook.AuditExampleTest do
  # examples/audit_hook.exs, the quick start of docs/README.md, does what the
  # page says it does. It starts serving as soon as it is loaded, so it runs
  # here as rued runs it: a child process spoken to on its stdio.
  use ExUnit.Case, async: true

  @example Path.expand("../examples/audit_hook.exs", __DIR__)

  setup do
    dir = Path.join(System.tmp_dir!(), "rue-audit-example-#{System.unique_integer([:positive])}")
    File.mkdir_p!(dir)
    on_exit(fn -> File.rm_rf!(dir) end)
    {:ok, dir: dir}
  end

  defp start(log) do
    ebin = Path.join(Mix.Project.build_path(), "lib/rue_hook/ebin")

    port =
      Port.open({:spawn_executable, System.find_executable("elixir")}, [
        :binary,
        :exit_status,
        {:line, 1_000_000},
        args: ["-pa", ebin, @example],
        env: [{~c"RUE_AUDIT_LOG", String.to_charlist(log)}]
      ])

    registration = line(port)
    Port.command(port, ~s({"register":{"ok":true}}\n))
    {port, registration}
  end

  defp line(port) do
    receive do
      {^port, {:data, {:eol, text}}} -> JSON.decode!(text)
    after
      30_000 -> flunk("the hook said nothing")
    end
  end

  defp ask(port, request) do
    Port.command(port, JSON.encode!(request) <> "\n")
    line(port)
  end

  defp stop(port) do
    Port.close(port)
  end

  test "it registers for journal and notify, and appends each entry as one line", %{dir: dir} do
    log = Path.join(dir, "audit.ndjson")
    {port, registration} = start(log)
    assert registration["register"]["name"] == "audit"
    assert registration["register"]["kinds"] == ["journal", "notify"]

    for seq <- [1, 2] do
      assert ask(port, %{
               "id" => seq,
               "kind" => "journal",
               "op" => "append",
               "entry" => %{"seq" => seq}
             }) ==
               %{"id" => seq, "ok" => true}
    end

    assert ask(port, %{
             "id" => 3,
             "kind" => "notify",
             "op" => "deliver",
             "level" => "warn",
             "subject" => "plan held",
             "body" => "waiting for approval"
           }) == %{"id" => 3, "ok" => true}

    stop(port)

    seqs =
      log |> File.read!() |> String.split("\n", trim: true) |> Enum.map(&JSON.decode!(&1)["seq"])

    assert seqs == [1, 2]
  end

  test "an entry it cannot record is refused with the reason", %{dir: dir} do
    {port, _} = start(Path.join([dir, "no-such-dir", "audit.ndjson"]))

    reply =
      ask(port, %{"id" => 1, "kind" => "journal", "op" => "append", "entry" => %{"seq" => 1}})

    stop(port)
    assert reply["ok"] == false
    assert reply["error"] =~ "is not writable"
  end
end
