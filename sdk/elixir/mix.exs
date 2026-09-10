defmodule RueHook.MixProject do
  @moduledoc """
  The Elixir embedding SDK for rue's hook protocol (docs/ROADMAP.md 7.11).

  No dependencies: a hook answers lines of JSON, and the decoding is small
  enough that asking a host application to take a JSON library it did not
  choose would be the worse trade. T4's reactive host is the tenant this
  exists for.
  """
  use Mix.Project

  def project do
    [
      app: :rue_hook,
      version: "0.1.0",
      elixir: "~> 1.15",
      start_permanent: Mix.env() == :prod,
      deps: [],
      description:
        "A conformance-tested client of rue's hook protocol (docs/hook-protocol.md v1)."
    ]
  end

  def application do
    [extra_applications: [:logger]]
  end
end
