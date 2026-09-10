defmodule RueHook.Hooks do
  @moduledoc """
  A handler module per kind, and the dispatch from a request frame to it.

  The registration frame is built from the kinds you actually supplied, so
  a hook cannot register for one it does not serve -- the failure that
  produces is a plan binding to it and refusing at its first step, a long
  way from where the mistake was made.

  A handler returns `{:ok, value}` or `{:refuse, reason}`. A refusal is a
  hook saying no, on the record, and is not a fault; a handler that raises
  becomes one too, because an unanswered request is Silent and Silent
  tells the operator nothing about why.
  """

  alias RueHook.Proto

  defstruct journal: nil,
            inventory: nil,
            execute: nil,
            probe: nil,
            approval: nil,
            secrets: nil,
            notify: nil,
            scheduler: nil,
            filesystem: false,
            stdin_preamble: false

  @doc "The kinds served, in the order 7.5 lists them."
  def kinds(%__MODULE__{} = h),
    do: Enum.filter(Proto.kinds(), fn k -> Map.get(h, String.to_atom(k)) != nil end)

  @doc """
  Answer one request frame.

  The reply always carries the request's id, including for an op this hook
  has no handler for: a request left unanswered is Silent, and the engine
  can only report that as "the step did not happen".
  """
  def answer(%__MODULE__{} = h, request) when is_map(request) do
    id = Map.get(request, "id")
    kind = Map.get(request, "kind", "")
    op = Map.get(request, "op", "")

    case Proto.find(kind, op) do
      nil ->
        refusal(id, "#{kind}.#{op} is not an op of this protocol")

      row ->
        case dispatch(h, row, request) do
          {:ok, fields} -> built(id, row, fields)
          {:refuse, why} -> refusal(id, why)
        end
    end
  end

  defp built(id, row, fields) do
    reply = Map.merge(%{"id" => id, "ok" => true}, fields)

    case Enum.reject(row.required_reply, &Map.has_key?(reply, &1)) do
      [] ->
        reply

      missing ->
        # Replies are built from the op's row, so this is a bug in the SDK
        # and not an R0303 for the far end to puzzle over.
        refusal(id, "the SDK built a #{row.kind}.#{row.op} reply without #{Enum.join(missing, ", ")}")
    end
  end

  defp refusal(id, why), do: %{"id" => id, "ok" => false, "error" => to_string(why)}

  defp dispatch(h, row, r) do
    case Map.get(h, String.to_atom(row.kind)) do
      nil -> {:refuse, "this hook does not serve #{row.kind}.#{row.op}"}
      mod -> call(mod, row, r)
    end
  rescue
    e -> {:refuse, "#{inspect(e.__struct__)}: #{Exception.message(e)}"}
  end

  defp s(r, k), do: Map.get(r, k) || ""

  defp call(mod, %{kind: "journal"}, r) do
    case Map.get(r, "entry") do
      nil -> {:refuse, "journal.append without an entry"}
      entry -> mod.append(entry) |> nofields()
    end
  end

  defp call(mod, %{kind: "inventory"}, _r), do: mod.list() |> field("hosts")

  defp call(mod, %{kind: "probe"}, r),
    do: mod.observe(s(r, "host"), s(r, "probe")) |> field("fact")

  defp call(mod, %{kind: "notify"}, r),
    do: mod.deliver(s(r, "level"), s(r, "subject"), s(r, "body")) |> nofields()

  defp call(mod, %{kind: "approval", op: "authenticators"}, _r),
    do: mod.authenticators() |> field("authenticators")

  defp call(mod, %{kind: "approval", op: "challenge"}, r),
    do:
      mod.challenge(s(r, "instance"), s(r, "digest"), Map.get(r, "scope"), Map.get(r, "context"))
      |> field("challenge")

  defp call(mod, %{kind: "approval", op: "verify"}, r) do
    case mod.verify(
           s(r, "instance"),
           s(r, "digest"),
           Map.get(r, "scope"),
           s(r, "authenticator"),
           s(r, "proof")
         ) do
      {:ok, {verified, reason}} -> {:ok, %{"verified" => verified, "reason" => reason}}
      other -> other
    end
  end

  defp call(mod, %{kind: "secrets", op: "resolve"}, r),
    do: mod.resolve(s(r, "ref")) |> field("value")

  defp call(mod, %{kind: "secrets", op: "deliver"}, r) do
    case mod.deliver(s(r, "instance"), s(r, "label"), s(r, "value")) do
      {:ok, {accepted, receipt}} -> {:ok, %{"accepted" => accepted, "receipt" => receipt}}
      other -> other
    end
  end

  defp call(mod, %{kind: "scheduler", op: "present"}, r),
    do: mod.present(s(r, "host"), s(r, "artifact")) |> field("present")

  defp call(mod, %{kind: "scheduler", op: op}, r) do
    host = s(r, "host")
    artifact = s(r, "artifact")
    deadline = Map.get(r, "deadline")

    case op do
      "install" -> mod.install(host, artifact)
      "arm" -> mod.arm(host, artifact, deadline)
      "rearm" -> mod.rearm(host, artifact, deadline)
      "disarm" -> mod.disarm(host, artifact)
    end
    |> nofields()
  end

  defp call(mod, %{kind: "execute", op: "run"}, r),
    do: mod.run(s(r, "host"), s(r, "instance"), Map.get(r, "body") || []) |> field("output")

  defp call(mod, %{kind: "execute", op: "read_fact"}, r) do
    case mod.read_fact(s(r, "host"), s(r, "shape")) do
      # nil is "no such file", which is an answer; an empty string would be
      # a file that exists and is empty.
      {:ok, nil} -> {:ok, %{}}
      {:ok, content} -> {:ok, %{"content" => content}}
      other -> other
    end
  end

  defp call(mod, %{kind: "execute", op: "bootstrap_state"}, r),
    do: mod.bootstrap_state(s(r, "host")) |> field("state")

  defp call(mod, %{kind: "execute", op: "clock"}, r),
    do: mod.clock(s(r, "host")) |> field("epoch_s")

  defp call(mod, %{kind: "execute", op: "instance_dir_list"}, r),
    do: mod.instance_dir_list(s(r, "host")) |> field("dirs")

  defp call(mod, %{kind: "execute", op: "get_file"}, r),
    do: mod.get_file(s(r, "host"), s(r, "instance"), s(r, "rel")) |> field("content")

  defp call(mod, %{kind: "execute", op: op}, r) do
    host = s(r, "host")
    inst = s(r, "instance")

    case op do
      "instance_dir_create" -> mod.instance_dir_create(host, inst)
      "instance_dir_remove" -> mod.instance_dir_remove(host, inst)
      "put_file" -> mod.put_file(host, inst, s(r, "rel"), s(r, "content"), Map.get(r, "mode") || 0)
      "replace_file" -> mod.replace_file(host, inst, s(r, "rel"), s(r, "content"))
      "remove_file" -> mod.remove_file(host, inst, s(r, "rel"))
      "host_lock" -> mod.host_lock(host)
    end
    |> nofields()
  end

  defp nofields({:ok, _}), do: {:ok, %{}}
  defp nofields(:ok), do: {:ok, %{}}
  defp nofields(other), do: other

  defp field({:ok, v}, name), do: {:ok, %{name => v}}
  defp field(other, _name), do: other
end
