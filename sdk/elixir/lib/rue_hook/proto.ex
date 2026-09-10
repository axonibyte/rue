defmodule RueHook.Proto do
  @moduledoc """
  The hook protocol's wire, as data (docs/hook-protocol.md v1).

  A transcription of `hook-proto/src/op.rs`, which is the protocol's one
  source. `tools/lint-hook-ops.sh`, a gate phase, checks this against that
  source in both directions, so the two cannot drift.
  """

  alias RueHook.Op

  @hook_protocol 1

  @kinds ~w(journal inventory execute probe approval secrets notify scheduler)

  @ops [
    %Op{kind: "journal", op: "append", request: ["entry"]},
    %Op{kind: "inventory", op: "list", required_reply: ["hosts"]},
    %Op{
      kind: "execute",
      op: "run",
      request: ["host", "instance", "body", "env", "secrets"],
      required_reply: ["output"],
      optional_reply: ["facts"],
      secret: :to_hook
    },
    %Op{kind: "execute", op: "read_fact", request: ["host", "shape"], optional_reply: ["content"]},
    %Op{kind: "execute", op: "bootstrap_state", request: ["host"], required_reply: ["state"]},
    %Op{
      kind: "execute",
      op: "clock",
      request: ["host"],
      required_reply: ["epoch_s"],
      optional: true
    },
    %Op{kind: "execute", op: "instance_dir_create", request: ["host", "instance"]},
    %Op{kind: "execute", op: "instance_dir_remove", request: ["host", "instance"]},
    %Op{kind: "execute", op: "instance_dir_list", request: ["host"], required_reply: ["dirs"]},
    %Op{
      kind: "execute",
      op: "put_file",
      request: ["host", "instance", "rel", "content", "mode"]
    },
    %Op{kind: "execute", op: "replace_file", request: ["host", "instance", "rel", "content"]},
    %Op{
      kind: "execute",
      op: "get_file",
      request: ["host", "instance", "rel"],
      required_reply: ["content"]
    },
    %Op{kind: "execute", op: "remove_file", request: ["host", "instance", "rel"]},
    %Op{kind: "execute", op: "host_lock", request: ["host"]},
    %Op{kind: "probe", op: "observe", request: ["host", "probe"], required_reply: ["fact"]},
    %Op{kind: "approval", op: "authenticators", required_reply: ["authenticators"]},
    %Op{
      kind: "approval",
      op: "challenge",
      request: ["instance", "digest", "scope", "context"],
      required_reply: ["challenge"]
    },
    %Op{
      kind: "approval",
      op: "verify",
      request: ["instance", "digest", "scope", "authenticator", "proof"],
      required_reply: ["verified"],
      optional_reply: ["reason"]
    },
    %Op{
      kind: "secrets",
      op: "resolve",
      request: ["ref"],
      required_reply: ["value"],
      secret: :to_engine
    },
    %Op{
      kind: "secrets",
      op: "deliver",
      request: ["instance", "label", "value"],
      required_reply: ["accepted"],
      optional_reply: ["receipt"],
      secret: :to_hook
    },
    %Op{kind: "notify", op: "deliver", request: ["level", "subject", "body"]},
    %Op{kind: "scheduler", op: "install", request: ["host", "artifact", "deadline"]},
    %Op{kind: "scheduler", op: "arm", request: ["host", "artifact", "deadline"]},
    %Op{kind: "scheduler", op: "rearm", request: ["host", "artifact", "deadline"]},
    %Op{kind: "scheduler", op: "disarm", request: ["host", "artifact", "deadline"]},
    %Op{
      kind: "scheduler",
      op: "present",
      request: ["host", "artifact", "deadline"],
      required_reply: ["present"]
    }
  ]

  def hook_protocol, do: @hook_protocol
  def kinds, do: @kinds
  def ops, do: @ops

  @doc "The op by kind and name, or nil for a pair the protocol has no row for."
  def find(kind, op), do: Enum.find(@ops, &(&1.kind == kind and &1.op == op))
end
