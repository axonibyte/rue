defmodule RueHook.ResolvedTest do
  # 7.11: a run's secrets reach the handler, and cannot be formatted onto a
  # command line or into a log by accident. This SDK handed handlers the
  # wire map, and its own docs warned that inspect/1 showed the secret.
  use ExUnit.Case, async: true

  alias RueHook.{Hooks, Resolved}

  @pw "correct-horse-battery"

  test "a secret is redacted wherever it is formatted" do
    r = %Resolved{text: @pw, secret: true}

    for shown <- [inspect(r), "#{r}", to_string(r), inspect(%{"k" => [r]}), JSON.encode!(%{"k" => r})] do
      refute shown =~ @pw, shown
    end

    assert RueHook.expose(r) == @pw
  end

  test "a value that is not secret formats as its text" do
    r = %Resolved{text: "plain", secret: false}
    assert "#{r}" == "plain"
    assert RueHook.expose(r) == "plain"
  end

  defmodule Recorder do
    def run(_host, _instance, body) do
      send(self(), {:body, body})
      {:ok, %{"stdout" => "", "outputs" => %{}}}
    end
  end

  test "a run handler receives the body's secrets as Resolved values" do
    wire = [
      %{
        "run" => %{
          "cmd" => %{"text" => "deploy", "secret" => false},
          "env" => [["PW", %{"text" => @pw, "secret" => true}]]
        }
      }
    ]

    reply =
      Hooks.answer(%Hooks{execute: Recorder}, %{
        "id" => 1, "kind" => "execute", "op" => "run", "host" => "h", "instance" => "i", "body" => wire
      })

    assert reply["ok"] == true, inspect(reply)
    assert_received {:body, body}
    refute inspect(body) =~ @pw
    [%{"run" => prim}] = body
    [["PW", pw]] = prim["env"]
    assert %Resolved{secret: true} = pw
    assert RueHook.expose(pw) == @pw
    assert RueHook.expose(prim["cmd"]) == "deploy"
    assert RueHook.carries_secret?(prim)
  end
end
