# The reference conformance hook for the Elixir SDK.
#
# docs/sdk-conformance.md's fixed world, served over stdio: what
# `rue sdk-conform` is pointed at to judge this SDK, and the worked
# example a host application copies.
#
# The exception is the four provocations of the `probe` kind, which
# deliberately violate the protocol. Those cannot go through
# `Hooks.answer/2`, because it builds replies from the op's own row and an
# `ok: true` without a required field is not expressible -- that is the
# guarantee the SDK exists for. So this file drops to the wire for exactly
# those, and for nothing else.
#
#   elixir -pa _build/dev/lib/rue_hook/ebin examples/conformance_hook.exs conform

defmodule Conform.World do
  @moduledoc "Every kind over the contract's fixed world, deliberately stateless."

  # journal
  def append(_entry), do: :ok

  # inventory
  def list do
    {:ok,
     [
       %{
         "name" => "conform-full",
         "address" => "198.51.100.7",
         "os" => "freebsd",
         "roles" => ["a", "b"],
         "reach" => ["hook"],
         "filesystem" => true,
         "stdin_preamble" => false,
         "scheduler" => "cron",
         "rue_root" => "/var/db/rue",
         "artifact" => "python",
         "facts" => %{"site" => "west"}
       },
       # Only what Appendix C requires; the rest takes its default.
       %{"name" => "conform-bare", "os" => "linux"}
     ]}
  end

  # execute
  def run(_host, _instance, body) do
    [%{"run" => prim} | _] = body
    env = Map.new(prim["env"] || [], fn [k, v] -> {k, v} end)
    pw = Map.get(env, "PW")

    if pw == nil do
      {:refuse, "the contract's run carries PW"}
    else
      {:ok,
       %{
         "stdout" => "ran #{length(body)} primitive\n",
         # `execute.run` carries a secret in both directions (7.5). An SDK
         # that scrubbed it on the way in, or could not reach it, fails here.
         "outputs" => %{
           "echo" => RueHook.expose(prim["cmd"]),
           "secret" => RueHook.expose(pw)
         }
       }}
    end
  end

  def read_fact(_host, "file:/conformance/present"), do: {:ok, "present\n"}
  def read_fact(_host, _shape), do: {:ok, nil}

  def bootstrap_state(_host),
    do:
      {:ok,
       %{
         "rue_root" => true,
         "group" => true,
         "instances_dir" => true,
         "lock" => true,
         "modes_ok" => true
       }}

  def clock(_host), do: {:ok, 1_700_000_000}
  def instance_dir_create(_host, _instance), do: :ok
  def instance_dir_remove(_host, _instance), do: :ok

  def instance_dir_list(_host),
    do: {:ok, [%{"instance" => "conform-1", "armed" => true, "fired" => false, "modes_ok" => true}]}

  def put_file(_h, _i, _rel, _content, _mode), do: :ok
  def replace_file(_h, _i, _rel, _content), do: :ok
  def get_file(_h, _i, _rel), do: {:ok, "1700000000\n"}
  def remove_file(_h, _i, _rel), do: :ok
  def host_lock(_host), do: :ok

  # probe
  def observe(_host, "conform-yes"), do: {:ok, RueHook.observation(:yes, "yes")}
  def observe(_host, "conform-no"), do: {:ok, RueHook.observation(:no, "no")}
  def observe(_host, "conform-unknown"), do: {:ok, RueHook.observation(:unknown, "")}

  def observe(_host, "conform-refuse"),
    do: {:refuse, "refused as the conformance contract asks, with a reason to read"}

  def observe(_host, other), do: {:refuse, "no probe named #{other}"}

  # approval
  def authenticators,
    do:
      {:ok,
       [
         %{"id" => "conform-human", "human" => true},
         %{"id" => "conform-machine", "human" => false}
       ]}

  def challenge(instance, digest, scope, _context),
    do: {:ok, "approve #{digest} on #{instance} (#{scope_text(scope)})"}

  def verify(_instance, digest, scope, _authenticator, proof) do
    # Bound to the digest *and* the scope (5.11). Built from what the
    # request carries, never from anything remembered, which is what makes
    # a replay fail.
    want = "#{digest}/#{scope_text(scope)}"

    if proof == want do
      {:ok, {true, ""}}
    else
      {:ok, {false, "the proof was made for another request or another scope"}}
    end
  end

  # `plan`, `step/<n>`, `ack/<n>` -- the scope as the contract spells it.
  defp scope_text("plan"), do: "plan"
  defp scope_text(%{"step" => n}), do: "step/#{n}"
  defp scope_text(%{"ack" => n}), do: "ack/#{n}"
  defp scope_text(_), do: "unknown"

  # secrets
  def resolve(_ref), do: {:ok, "conformance-resolved-secret"}

  # Declining is an answer, not a refusal: the engine offers the secret to
  # the next acceptor.
  def deliver(_instance, "unwanted", _value), do: {:ok, {false, "receipt-unwanted"}}
  def deliver(_instance, label, _value), do: {:ok, {true, "receipt-#{label}"}}

  # scheduler
  def install(_host, _artifact), do: :ok
  def arm(_host, _artifact, _deadline), do: :ok
  def rearm(_host, _artifact, _deadline), do: :ok
  def disarm(_host, _artifact), do: :ok
  def present(_host, "conform-present.sh"), do: {:ok, true}
  def present(_host, "conform-absent.sh"), do: {:ok, false}
  # Never guess: the engine reads `false` as "install it again".
  def present(_host, _artifact), do: {:ok, "unknown"}
end

defmodule Conform.Notifier do
  @moduledoc "notify's `deliver` collides with secrets', so it gets its own module."
  def deliver(_level, _subject, _body), do: :ok
end

defmodule Conform.Main do
  @provocations ~w(conform-missing-field conform-no-ok conform-silent)

  def run(name) do
    hooks = %RueHook.Hooks{
      journal: Conform.World,
      inventory: Conform.World,
      execute: Conform.World,
      probe: Conform.World,
      approval: Conform.World,
      secrets: Conform.World,
      notify: Conform.Notifier,
      scheduler: Conform.World,
      filesystem: true,
      stdin_preamble: true
    }

    IO.puts(JSON.encode!(%{"register" => RueHook.Serve.registration(name, hooks)}))

    case IO.gets("") do
      :eof ->
        System.halt(1)

      line ->
        case JSON.decode(String.trim(line)) do
          {:ok, %{"register" => %{"ok" => true}}} -> pump(hooks)
          _ -> System.halt(1)
        end
    end
  end

  defp pump(hooks) do
    case IO.gets("") do
      :eof ->
        :ok

      line ->
        handle(String.trim(line), hooks)
        pump(hooks)
    end
  end

  defp handle("", _hooks), do: :ok

  defp handle(line, hooks) do
    case JSON.decode(line) do
      {:ok, %{"kind" => "probe", "probe" => p} = frame} when p in @provocations ->
        provoke(p, Map.get(frame, "id"))

      {:ok, %{"kind" => _} = frame} ->
        IO.puts(JSON.encode!(RueHook.Hooks.answer(hooks, frame)))

      _ ->
        :ok
    end
  end

  # Deliberately malformed, and deliberately not through the SDK.
  defp provoke("conform-missing-field", id), do: IO.puts(JSON.encode!(%{"id" => id, "ok" => true}))
  defp provoke("conform-no-ok", id), do: IO.puts(JSON.encode!(%{"id" => id}))
  # Say nothing at all: the engine reads that as Silent.
  defp provoke("conform-silent", _id), do: :ok
end

Conform.Main.run(List.first(System.argv()) || "conform")
