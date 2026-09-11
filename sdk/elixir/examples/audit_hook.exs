# An audit hook: a journal sink that keeps every entry, and a notifier.
#
# Bind it in a site with `journal to: local(), hook(:audit)` and
# `notify via: hook(:audit)`, and have rued spawn it:
#
#     rued run --spawn audit="elixir -pa _build/prod/lib/rue_hook/ebin audit_hook.exs" ...
#
# Each journal entry is appended to $RUE_AUDIT_LOG (default audit.ndjson)
# as one line of JSON. A sink that cannot record an entry must say so: the
# engine then refuses to proceed (R0304) rather than run a step nobody
# recorded. Notifications go to stderr, because stdout carries the protocol.

defmodule AuditHook.Log do
  def append(entry) do
    path = System.get_env("RUE_AUDIT_LOG", "audit.ndjson")

    case File.write(path, JSON.encode!(entry) <> "\n", [:append]) do
      :ok ->
        :ok

      {:error, why} ->
        {:refuse, "the audit log #{path} is not writable: #{:file.format_error(why)}"}
    end
  end
end

defmodule AuditHook.Stderr do
  def deliver(level, subject, body) do
    IO.puts(:stderr, "[#{level}] #{subject}: #{body}")
    :ok
  end
end

RueHook.Serve.stdio("audit", %RueHook.Hooks{journal: AuditHook.Log, notify: AuditHook.Stderr})
