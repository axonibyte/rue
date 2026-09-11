defmodule RueHook.Serve do
  @moduledoc """
  The way `rued` reaches a hook it spawned (docs/hook-protocol.md, "A hook
  over stdio").

  Send the registration frame, read the acknowledgement, then answer one
  request per line until stdin closes. Event frames from a subscription are
  skipped: a hook that is not also an operator has nothing to do with them.
  """

  alias RueHook.{Hooks, Proto}

  def registration(name, %Hooks{} = h) do
    %{
      "name" => name,
      "kinds" => Hooks.kinds(h),
      "protocol" => Proto.hook_protocol(),
      "filesystem" => h.filesystem,
      "stdin_preamble" => h.stdin_preamble
    }
  end

  @doc """
  Serve as a child the daemon spawned (`rued run --spawn`).

  The registration frame is the first line of stdout, before anything
  else, so keep your own logging on stderr.
  """
  def stdio(name, %Hooks{} = h, opts \\ []) do
    write(:stdio, %{"register" => registration(name, h)})

    case IO.gets("") do
      :eof ->
        refuse("no acknowledgement from rued")

      line ->
        case JSON.decode(String.trim(line)) do
          {:ok, %{"register" => %{"ok" => true}}} -> pump(:stdio, :stdio, h, Keyword.get(opts, :budget_ms))
          _ -> refuse("registration was refused: #{String.trim(line)}")
        end
    end
  end

  @doc """
  Answer one request per line of `input` until it ends, writing replies to
  `output`: the loop `stdio/3` runs after registering. Blank lines, lines
  that are not a JSON object, event frames and frames with no `kind` get no
  reply; every request gets exactly one, within `budget_ms` if given (see
  `RueHook.Hooks.answer_within/3`).
  """
  def pump(input, output, %Hooks{} = h, budget_ms) do
    case IO.gets(input, "") do
      :eof ->
        :ok

      {:error, _} ->
        :ok

      line ->
        line
        |> String.trim()
        |> answer_line(output, h, budget_ms)

        pump(input, output, h, budget_ms)
    end
  end

  defp answer_line("", _output, _h, _budget), do: :ok

  defp answer_line(line, output, h, budget_ms) do
    case JSON.decode(line) do
      {:ok, %{"kind" => _} = frame} ->
        if Map.has_key?(frame, "event"), do: :ok, else: write(output, Hooks.answer_within(h, frame, budget_ms))

      _ ->
        :ok
    end
  end

  # IO.puts writes the line and the newline together; an unflushed reply is
  # a silence, and a silence is a refusal with nothing to say about why.
  @doc false
  def write(output, frame), do: IO.puts(output, encode(frame))

  @doc """
  A frame as one line of JSON. A reply JSON cannot encode -- a tuple a
  handler put in it -- is refused by name rather than ending the loop, or
  the connection of the host that holds it.
  """
  def encode(frame) do
    JSON.encode!(frame)
  rescue
    e ->
      JSON.encode!(
        Hooks.refusal_for(Map.get(frame, "id"), "the reply could not be written: #{Exception.message(e)}")
      )
  end

  defp refuse(why) do
    IO.puts(:stderr, "rue_hook: " <> why)
    System.halt(1)
  end
end
