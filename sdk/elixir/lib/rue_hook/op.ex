defmodule RueHook.Op do
  @moduledoc """
  One op of the hook protocol: what its request carries and what its reply
  must (docs/hook-protocol.md v1).

  Its own module because Elixir cannot use a struct in a module attribute
  of the context that defines it, and `RueHook.Proto`'s table is exactly
  that.
  """

  defstruct kind: "",
            op: "",
            request: [],
            #: Fields an `ok: true` reply must carry; R0303 without them.
            required_reply: [],
            optional_reply: [],
            #: `:to_hook` or `:to_engine` for the four messages of 7.5 that
            #: may carry a secret; nil for every other.
            secret: nil,
            #: True only for `execute.clock`, which a hook may decline.
            optional: false
end
