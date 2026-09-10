# T4's reactive host (docs/ROADMAP.md 8.4), on the Elixir SDK.
#
# One process, one connection, three roles at once: it registers the hooks
# the engine calls back into, it acts as the declared operator
# `:reactive_host`, and it subscribes to its own plans. That is what 7.11
# means by "a GenServer owning the socket", and it is the case the control
# channel was designed for -- a hook connection may also be an operator
# (engine/tests/control.rs, "T4's shape").
#
# The state machine is the point of the tenant: entering `:shedding` fires
# the plan, leaving it recants. Nothing about that is scheduled or
# time-driven; the host decides, and rue is how the decision is carried out
# reversibly.
#
#   elixir -pa <ebin> reactive_host.exs <socket> <command> [args]
#
# Commands, each one thing the harness needs to observe:
#   enter <plan-file> <plan>   fire the plan, print its instance id
#   leave <instance>           recant it
#   force <instance>           recant with --force=drift, after DriftHeld
#   watch <instance> <secs>    print the events this host was sent
#   rogue-register             try to register a name outside may_register
#   rogue-verb <instance>      try a verb outside operator_for

defmodule Actuator do
  @moduledoc """
  The appliance the plan sheds load on: three actuators whose state is a
  file the harness can read and hand-edit, which is how the drift cases are
  staged.
  """

  def state_dir, do: System.get_env("RUE_T4_STATE") || "/tmp/rue-t4-state"

  def read(name) do
    case File.read(Path.join(state_dir(), name)) do
      {:ok, text} -> String.trim(text)
      _ -> "on"
    end
  end

  def write(name, value) do
    File.mkdir_p!(state_dir())
    File.write!(Path.join(state_dir(), name), value <> "\n")
  end

  # execute: the engine's `hook(:host_actuate, set: %{...})` primitive.
  def run(_host, _instance, body) do
    for prim <- body, %{"hook" => h} = prim do
      for {k, v} <- Map.get(h, "args", %{}) || %{}, is_binary(k) do
        write(k, RueHook.expose(v))
      end
    end

    {:ok, %{"stdout" => "", "outputs" => %{}}}
  end

  # The engine reads a modified fact back to compare it: `actuator.state(x)`.
  def read_fact(_host, shape) do
    case String.split(shape, ":") do
      ["actuator", "state", name] -> {:ok, read(name)}
      _ -> {:ok, nil}
    end
  end

  def bootstrap_state(_host),
    do:
      {:ok,
       %{
         "rue_root" => false,
         "group" => false,
         "instances_dir" => false,
         "lock" => false,
         "modes_ok" => false
       }}

  def clock(_host), do: {:ok, System.system_time(:second)}
end

defmodule HostLog do
  @moduledoc "journal to: hook(:host_log). The host keeps its own record."

  def append(entry) do
    path = Path.join(Actuator.state_dir(), "journal.ndjson")
    File.mkdir_p!(Actuator.state_dir())
    File.write!(path, JSON.encode!(entry) <> "\n", [:append])
    :ok
  end
end

defmodule Host do
  def main([socket | rest]) do
    {:ok, c} =
      RueHook.Client.start_link(
        socket: socket,
        identity: "reactive_host",
        events_to: self()
      )

    :ok = RueHook.Client.register(c, "host_log", %RueHook.Hooks{journal: HostLog})

    :ok =
      RueHook.Client.register(c, "host_actuate", %RueHook.Hooks{
        execute: Actuator,
        stdin_preamble: false
      })

    run(c, rest)
  end

  # Entering the state: fire the plan. The instance id goes to stdout so
  # the harness can drive the rest.
  defp run(c, ["enter", file, plan]) do
    ir = plan_ir(file, plan)

    case RueHook.Client.call(c, "apply", %{"ir" => ir, "params" => %{}}) do
      {:ok, result} -> IO.puts(result["id"] <> " " <> result["state"])
      {:error, e} -> die("apply refused: #{inspect(e)}")
    end
  end

  # Leaving the state: recant.
  defp run(c, ["leave", id]), do: verb(c, "recant", %{"instance" => id})

  # A hand-flipped actuator left the instance DriftHeld; the host forces it
  # through, over the same channel it applied on.
  defp run(c, ["force", id]),
    do: verb(c, "recant", %{"instance" => id, "force" => "drift"})

  defp run(c, ["status", id]), do: verb(c, "status", %{"instance" => id})

  # Print the events this host was sent for its plans, for `secs` seconds.
  defp run(_c, ["watch", secs]) do
    collect(String.to_integer(secs) * 1000)
    |> Enum.each(fn e -> IO.puts(JSON.encode!(e)) end)
  end

  # A name outside `may_register`: R0505.
  defp run(c, ["rogue-register"]) do
    case RueHook.Client.register(c, "not_declared", %RueHook.Hooks{journal: HostLog}) do
      :ok -> die("a name outside may_register was accepted")
      {:error, e} -> IO.puts("refused " <> JSON.encode!(e))
    end
  end

  # A verb outside `operator_for`: R0504. `bootstrap` is an admin verb on a
  # host, which this identity is not scoped to.
  defp run(c, ["rogue-verb"]) do
    case RueHook.Client.call(c, "bootstrap", %{"host" => "elsewhere"}) do
      {:ok, _} -> die("a verb outside operator_for was accepted")
      {:error, e} -> IO.puts("refused " <> JSON.encode!(e))
    end
  end

  defp verb(c, name, args) do
    case RueHook.Client.call(c, name, args) do
      {:ok, result} -> IO.puts(JSON.encode!(result))
      {:error, e} -> die("#{name} refused: #{inspect(e)}")
    end
  end

  defp collect(ms), do: collect(ms, System.monotonic_time(:millisecond), [])

  defp collect(ms, started, acc) do
    left = ms - (System.monotonic_time(:millisecond) - started)

    if left <= 0 do
      Enum.reverse(acc)
    else
      receive do
        {:rue_event, entry} -> collect(ms, started, [entry | acc])
      after
        left -> Enum.reverse(acc)
      end
    end
  end

  # The plan IR. Resolving .rue text needs the front end and the front end
  # is Rust, so a host in another language asks the CLI for the IR rather
  # than linking it -- `rue check --ir` exists for exactly this (7.11).
  defp plan_ir(file, plan) do
    rue = System.get_env("RUE_BIN") || "rue"

    {out, 0} =
      System.cmd(rue, [
        "check",
        file,
        "--ir",
        "--host",
        "site-ctl",
        "--plan-name",
        plan,
        "--inventory",
        Path.join(Path.dirname(file), "inventory.toml")
      ])

    JSON.decode!(out)
  end

  defp die(why) do
    IO.puts(:stderr, "reactive-host: " <> why)
    System.halt(1)
  end
end

Host.main(System.argv())
