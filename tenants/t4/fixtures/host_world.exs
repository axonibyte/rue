# T4's inventory hook, as a daemon-spawned child.
#
# It is a child and not part of the reactive host's own connection because
# an `inventory from: hook()` is asked once, at boot, before the daemon
# serves its socket -- so nothing registered over the socket can answer it
# (docs/hook-protocol.md, "The engine asks inventory.list once").
#
# What it reports is the host itself: one controller, reachable only
# through the actuate transport, with no filesystem the engine may write.

defmodule HostWorld do
  def list do
    {:ok,
     [
       %{
         "name" => "site-ctl",
         "address" => "127.0.0.1",
         "os" => "reactive-host",
         "roles" => ["controller"],
         "reach" => ["actuate"],
         "filesystem" => false
       }
     ]}
  end
end

RueHook.Serve.stdio("host_world", %RueHook.Hooks{inventory: HostWorld})
