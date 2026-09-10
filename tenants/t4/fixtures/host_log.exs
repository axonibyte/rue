# T4's journal hook, as a daemon-spawned child.
#
# It is a child and not part of the reactive host's own connection for the
# same reason host_world is: boot recovery writes to the journal before the
# daemon serves its socket, so a sink that means to connect over that
# socket has not registered yet and R0304 refuses the entry. `rued` now
# says so at startup rather than failing on entry 1.
#
# The reactive host still sees every entry -- through its subscription,
# which is the path meant for that. A journal sink is where entries are
# durably kept; a subscription is how a host watches them.

defmodule HostLog do
  def state_dir, do: System.get_env("RUE_T4_STATE") || "/tmp/rue-t4-state"

  def append(entry) do
    File.mkdir_p!(state_dir())
    File.write!(Path.join(state_dir(), "journal.ndjson"), JSON.encode!(entry) <> "\n", [:append])
    :ok
  end
end

RueHook.Serve.stdio("host_log", %RueHook.Hooks{journal: HostLog})
