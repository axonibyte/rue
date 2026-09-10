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
  def stdio(name, %Hooks{} = h) do
    write(%{"register" => registration(name, h)})

    case IO.gets("") do
      :eof ->
        refuse("no acknowledgement from rued")

      line ->
        case JSON.decode(String.trim(line)) do
          {:ok, %{"register" => %{"ok" => true}}} -> pump(h)
          _ -> refuse("registration was refused: #{String.trim(line)}")
        end
    end
  end

  defp pump(h) do
    case IO.gets("") do
      :eof ->
        :ok

      line ->
        line
        |> String.trim()
        |> answer_line(h)

        pump(h)
    end
  end

  defp answer_line("", _h), do: :ok

  defp answer_line(line, h) do
    case JSON.decode(line) do
      {:ok, frame} when is_map(frame) ->
        if Map.has_key?(frame, "kind"), do: write(Hooks.answer(h, frame)), else: :ok

      _ ->
        :ok
    end
  end

  # IO.puts writes the line and the newline together; an unflushed reply is
  # a silence, and a silence is a refusal with nothing to say about why.
  defp write(frame), do: IO.puts(JSON.encode!(frame))

  defp refuse(why) do
    IO.puts(:stderr, "rue_hook: " <> why)
    System.halt(1)
  end
end
