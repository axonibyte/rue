# A journal sink that can be told to say no, for the multi-sink refusal of
# Phase 4 task 6 (docs/ROADMAP.md 5.10, 1261).
#
# It refuses only while a flag file exists, and that is not a convenience:
# a sink refusing from its first entry stops the daemon before it starts,
# because registering a hook is itself journaled. The interesting case is
# a sink that was healthy when the site came up and says no later, which is
# what a full disk or a revoked token looks like from here.
#
# Everything it is asked for is recorded whether it accepts or refuses, so
# a test can tell "refused the entry" from "was never sent one" -- the
# whole question being whether both sinks really were delivered to.

defmodule RefusingLog do
  def state_dir, do: System.get_env("RUE_T4_STATE") || "/tmp/rue-t4-state"

  defp path(name), do: Path.join(state_dir(), name)

  def append(entry) do
    File.mkdir_p!(state_dir())
    File.write!(path("refusing-saw.ndjson"), JSON.encode!(entry) <> "\n", [:append])

    if File.exists?(path("refuse")) do
      {:refuse, "this sink is not accepting entries: its storage is read-only"}
    else
      :ok
    end
  end
end

RueHook.Serve.stdio("host_log_refusing", %RueHook.Hooks{journal: RefusingLog})
