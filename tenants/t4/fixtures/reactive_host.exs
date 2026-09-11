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

  # execute: the engine's resolved primitives. A hook's arguments arrive as
  # `[[name, {text, secret}], ...]`, and what an argument MEANS is the
  # appliance's business -- rue carries `set:` across verbatim and does not
  # pretend to understand an actuator map.
  def run(_host, _instance, body) do
    Enum.each(body, &apply_prim/1)
    {:ok, %{"stdout" => "", "outputs" => %{}}}
  end

  defp apply_prim(%{"hook" => h}) do
    for [name, value] <- Map.get(h, "args") || [], name == "set" do
      for {k, v} <- parse_set(RueHook.expose(value)), do: write(k, v)
    end
  end

  # A restore of a `modified` fact: the engine hands back what it read
  # before the step, one fact at a time, and the appliance puts it back.
  defp apply_prim(%{"write" => w}) do
    case String.split(Map.get(w, "shape", ""), ":") do
      ["actuator", "state", name] -> write(name, String.trim(RueHook.expose(w["content"])))
      _ -> :ok
    end
  end

  defp apply_prim(other) do
    # Never silently: a primitive this appliance does not implement is a
    # thing rue asked for and did not get.
    IO.puts(:stderr, "actuator: no idea what to do with " <> JSON.encode!(other))
  end

  # `%{"hvac-1": :off, "pump-1": :low}` as the tenant writes it.
  defp parse_set(text) do
    ~r/"([^"]+)":\s*:?([A-Za-z0-9_-]+)/
    |> Regex.scan(text)
    |> Enum.map(fn [_, k, v] -> {k, v} end)
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

defmodule Host do
  def main([socket | rest]) do
    {:ok, c} =
      RueHook.Client.start_link(
        socket: socket,
        identity: "reactive_host",
        events_to: self()
      )

    # Only the actuator: the journal and the inventory are spawned
    # children, because both are used before the socket is served and a
    # hook that connects over it cannot have registered by then. The host
    # still sees every entry through its subscription.
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
      # A refusal is an answer, and the code is what the harness asserts
      # on, so it goes to stdout rather than into an exit status.
      {:error, e} -> IO.puts("refused " <> JSON.encode!(e))
    end
  end

  # Leaving the state: recant.
  defp run(c, ["leave", id]), do: verb(c, "recant", %{"instance" => id})

  # A recant the engine is entitled to refuse -- a `:defer` step whose fact
  # was flipped by hand is R0103 until somebody forces it. The refusal is
  # the answer, so it goes to stdout the way `enter`'s does, rather than
  # into an exit status where the harness could only see that something
  # went wrong and not what.
  defp run(c, ["try-leave", id]) do
    case RueHook.Client.call(c, "recant", %{"instance" => id}) do
      {:ok, result} -> IO.puts(JSON.encode!(result))
      {:error, e} -> IO.puts("refused " <> JSON.encode!(e))
    end
  end

  # A request: gated and checked against the real daemon, nothing run and
  # nothing reserved (8.4, "request dry-run journaling only").
  defp run(c, ["rehearse", file, plan]) do
    ir = plan_ir(file, plan)

    case RueHook.Client.call(c, "apply", %{
           "ir" => ir,
           "params" => %{},
           "rehearsal" => true
         }) do
      {:ok, result} -> IO.puts(JSON.encode!(result))
      {:error, e} -> IO.puts("refused " <> JSON.encode!(e))
    end
  end

  # A hand-flipped actuator left the instance DriftHeld; the host forces it
  # through, over the same channel it applied on.
  defp run(c, ["force", id]),
    do: verb(c, "recant", %{"instance" => id, "force" => ["drift"]})

  defp run(c, ["status", id]), do: verb(c, "status", %{"instance" => id})

  # Print the events this host was sent for its plans, for `secs` seconds.
  defp run(_c, ["watch", secs]) do
    collect(String.to_integer(secs) * 1000)
    |> Enum.each(fn e -> IO.puts(JSON.encode!(e)) end)
  end

  # A name outside `may_register`: R0505.
  defp run(c, ["rogue-register"]) do
    case RueHook.Client.register(c, "not_declared", %RueHook.Hooks{execute: Actuator}) do
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
