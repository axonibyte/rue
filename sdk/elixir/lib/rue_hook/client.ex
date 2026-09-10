defmodule RueHook.Client do
  @moduledoc """
  A GenServer that owns one control-channel connection (7.11).

  This is the shape an embedded host wants and the reason the Elixir SDK
  exists: one process holds the socket and is simultaneously a hook the
  engine calls, an operator issuing verbs, and a subscriber receiving its
  plans' journal entries. Replies and events and hook requests all arrive
  on the same connection, interleaved, and something has to demultiplex
  them by id -- a GenServer is that something, without a thread or a lock.

      {:ok, c} = RueHook.Client.start_link(socket: path, identity: "reactive_host")
      :ok = RueHook.Client.register(c, "host_actuate", %RueHook.Hooks{execute: Actuator})
      {:ok, result} = RueHook.Client.call(c, "apply", %{"ir" => ir, "params" => %{}})

  Events are delivered to `:events_to` as `{:rue_event, entry}`. A host that
  subscribes to a plan and also drives it -- T4's shape exactly -- would
  otherwise lose the events it subscribed to whenever it happened to be
  mid-verb, so they are never dropped on the floor here.
  """

  use GenServer

  alias RueHook.{Hooks, Serve}

  @type option ::
          {:socket, String.t()}
          | {:identity, String.t() | nil}
          | {:events_to, pid() | nil}
          | {:name, GenServer.name()}

  @spec start_link([option]) :: GenServer.on_start()
  def start_link(opts) do
    {name, opts} = Keyword.pop(opts, :name)
    if name, do: GenServer.start_link(__MODULE__, opts, name: name), else: GenServer.start_link(__MODULE__, opts)
  end

  @doc "Register a hook on this connection. A connection may hold several."
  def register(client, name, %Hooks{} = hooks, timeout \\ 5_000),
    do: GenServer.call(client, {:register, name, hooks}, timeout)

  @doc "Issue a verb and wait for its reply."
  def call(client, verb, args, timeout \\ 30_000),
    do: GenServer.call(client, {:verb, verb, args}, timeout)

  @doc "Close the connection."
  def close(client), do: GenServer.stop(client, :normal)

  # --- the server ------------------------------------------------------

  @impl true
  def init(opts) do
    path = Keyword.fetch!(opts, :socket)
    identity = Keyword.get(opts, :identity)
    events_to = Keyword.get(opts, :events_to)

    case :gen_tcp.connect({:local, path}, 0, [:binary, active: false, packet: :line]) do
      {:ok, sock} ->
        state = %{
          sock: sock,
          identity: identity,
          events_to: events_to,
          # id -> from, for verbs awaiting a reply
          pending: %{},
          # name -> hooks, for requests the engine sends us
          hooks: %{},
          next_id: 1,
          registering: nil
        }

        case hello(state) do
          {:ok, state} ->
            reader(self(), sock)
            {:ok, state}

          {:error, why} ->
            {:stop, {:hello, why}}
        end

      {:error, why} ->
        {:stop, {:connect, why}}
    end
  end

  defp hello(%{sock: sock, identity: identity} = state) do
    send_frame(sock, %{"hello" => %{"proto" => 1, "identity" => identity}})

    case :gen_tcp.recv(sock, 0) do
      {:ok, line} ->
        case JSON.decode(String.trim(line)) do
          {:ok, %{"hello" => %{"ok" => true}}} -> {:ok, state}
          other -> {:error, other}
        end

      other ->
        {:error, other}
    end
  end

  # The reading half runs in its own process and forwards every line, so
  # the GenServer never blocks on the socket while it is answering a call.
  defp reader(owner, sock) do
    spawn_link(fn -> read_loop(owner, sock) end)
  end

  defp read_loop(owner, sock) do
    case :gen_tcp.recv(sock, 0) do
      {:ok, line} ->
        send(owner, {:line, String.trim(line)})
        read_loop(owner, sock)

      {:error, _closed} ->
        send(owner, :closed)
    end
  end

  @impl true
  def handle_call({:register, name, hooks}, from, state) do
    send_frame(state.sock, %{"register" => Serve.registration(name, hooks)})
    {:noreply, %{state | registering: {from, name, hooks}}}
  end

  def handle_call({:verb, verb, args}, from, state) do
    id = state.next_id
    send_frame(state.sock, %{"id" => id, "verb" => verb, "args" => args})
    {:noreply, %{state | next_id: id + 1, pending: Map.put(state.pending, id, from)}}
  end

  @impl true
  def handle_info({:line, line}, state) do
    case JSON.decode(line) do
      {:ok, frame} -> {:noreply, route(frame, state)}
      _ -> {:noreply, state}
    end
  end

  def handle_info(:closed, state) do
    # Every waiter learns the connection is gone rather than timing out.
    Enum.each(state.pending, fn {_id, from} -> GenServer.reply(from, {:error, :closed}) end)
    {:stop, :normal, %{state | pending: %{}}}
  end

  def handle_info(_other, state), do: {:noreply, state}

  # An acknowledgement of our own registration.
  defp route(%{"register" => reg}, %{registering: {from, name, hooks}} = state) do
    if reg["ok"] == true do
      GenServer.reply(from, :ok)
      %{state | registering: nil, hooks: Map.put(state.hooks, name, hooks)}
    else
      GenServer.reply(from, {:error, reg})
      %{state | registering: nil}
    end
  end

  # A journal entry for a plan this identity subscribes to.
  defp route(%{"event" => entry}, state) do
    if state.events_to, do: send(state.events_to, {:rue_event, entry})
    state
  end

  # A request from the engine to one of our hooks. Which hook is not on the
  # wire: the engine sends what it asked of the name it looked up, and a
  # connection holding several answers from whichever serves that kind.
  defp route(%{"kind" => kind} = request, state) do
    hooks =
      state.hooks
      |> Map.values()
      |> Enum.find(fn h -> Map.get(h, String.to_existing_atom(kind)) != nil end)

    reply =
      case hooks do
        nil ->
          %{
            "id" => Map.get(request, "id"),
            "ok" => false,
            "error" => "this connection serves no #{kind} hook"
          }

        h ->
          Hooks.answer(h, request)
      end

    send_frame(state.sock, reply)
    state
  end

  # A reply to a verb we issued.
  defp route(%{"id" => id} = frame, state) when is_integer(id) do
    case Map.pop(state.pending, id) do
      {nil, _} ->
        state

      {from, pending} ->
        answer =
          if frame["ok"] == true do
            {:ok, Map.get(frame, "result")}
          else
            {:error, Map.get(frame, "error")}
          end

        GenServer.reply(from, answer)
        %{state | pending: pending}
    end
  end

  defp route(_frame, state), do: state

  defp send_frame(sock, frame), do: :gen_tcp.send(sock, JSON.encode!(frame) <> "\n")
end
