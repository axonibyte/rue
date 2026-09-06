{-# LANGUAGE OverloadedStrings #-}
-- | The plan IR: the checked plan as data (docs/TESTING.md, "The plan IR").
--
-- What 'Rue.Proto.Check.check' consumes -- a site, a requester and one
-- concrete per-host plan -- as one JSON document, so that Phase 1's Rust
-- checker reads exactly the input the prototype checked and Phase 2's front
-- end has a target to produce. Written by hand, field by field, like
-- 'Rue.Proto.Verdict.toJson': a derived encoding would leak this module's
-- constructor shapes into a document other implementations must match.
--
-- Every duration is whole seconds under a name ending in @_s@. Sum types are
-- a bare string for a unit constructor and a one-key object otherwise; items
-- carry an @item@ tag with the step's fields flattened beside it. The
-- prototype's stand-in flags (@undo_closed@, @undo_idempotent@,
-- @undo_one_line@) are carried as they are; the unit that replaces them with
-- bodies bumps 'irVersion'.
module Rue.Proto.Json.PlanIr
  ( irVersion
  , toJson
  ) where

import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson.Key as Key
import Data.Text (Text)
import qualified Data.Vector as V
import Rue.Proto.Model

irVersion :: Int
irVersion = 1

toJson :: Site -> Text -> Plan -> Value
toJson site requester p =
  object
    [ "ir_version" .= irVersion
    , "requester" .= requester
    , "site" .= siteJson site
    , "plan" .= planJson p
    ]

siteJson :: Site -> Value
siteJson s =
  object
    [ "hosts" .= map hostJson (siteHosts s)
    , "transports" .= siteTransports s
    , "authenticators" .= [object ["id" .= authId a, "human" .= authHuman a] | a <- siteAuthenticators s]
    , "max_wait_s" .= fmap seconds (siteMaxWait s)
    , "scheduler_present" .= siteSchedulerPresent s
    ]

hostJson :: HostRecord -> Value
hostJson h =
  object
    [ "name" .= hrName h
    , "os" .= hrOs h
    , "reach" .= hrReach h
    , "filesystem" .= hrFilesystem h
    ]

planJson :: Plan -> Value
planJson p =
  object
    [ "id" .= planId p
    , "owner" .= planOwner p
    , "gate" .= fmap planGateJson (planGate p)
    , "wane_s" .= fmap seconds (planWane p)
    , "renew_within_s" .= fmap seconds (planRenewWithin p)
    , "backstop" .= fmap backstopJson (planBackstop p)
    , "fires_by_construction" .= planFiresByConstruction p
    , "strictness" .= (case planStrictness p of Strict -> "strict" :: Text; Warn -> "warn")
    , "mode" .= (case planMode p of Manual -> "manual" :: Text; Auto -> "auto")
    , "exclusivity" .= planExclusivity p
    , "require_journal" .= fmap (\j -> case j of Chained -> "chained" :: Text; Signed -> "signed") (planRequireJournal p)
    , "body" .= map itemJson (planBody p)
    ]

planGateJson :: PlanGate -> Value
planGateJson g =
  object
    [ "expr" .= gateJson (gateExpr g)
    , "window_s" .= fmap seconds (gateWindow g)
    , "allow_zero_human" .= allowZeroHuman g
    ]

backstopJson :: Backstop -> Value
backstopJson b =
  object
    [ "triggers" .= map triggerJson (triggers b)
    , "arm_before" .= (let ArmBefore n = armBefore b in n)
    ]

triggerJson :: Trigger -> Value
triggerJson t = case t of
  After d -> one "after" (Number (fromIntegral (seconds d)))
  UnlessConfirmed d -> one "unless_confirmed" (Number (fromIntegral (seconds d)))
  UnlessHeartbeat d i -> one "unless_heartbeat" (object ["deadline_s" .= seconds d, "interval_s" .= fmap seconds i])

gateJson :: GateExpr -> Value
gateJson g = case g of
  Thresh n fs -> one "thresh" (object ["n" .= n, "factors" .= map factorJson fs])
  Single f -> one "single" (factorJson f)

factorJson :: Factor -> Value
factorJson f = case f of
  Auth i w -> one "auth" (object ["id" .= i, "weight" .= w])
  Humans w -> one "humans" (object ["weight" .= w])
  Group inner w -> one "group" (object ["expr" .= gateJson inner, "weight" .= w])
  Wait d w -> one "wait" (object ["duration_s" .= seconds d, "weight" .= w])

guardJson :: Guard -> Value
guardJson g =
  object
    [ "name" .= guardName g
    , "value" .= triText (guardValue g)
    , "force_never" .= guardForceNever g
    ]

triText :: Tri -> Text
triText t = case t of
  Yes -> "yes"
  No -> "no"
  Unknown -> "unknown"

onLapseText :: OnLapse -> Text
onLapseText l = case l of
  LapseRevert -> "revert"
  LapseHold -> "hold"

itemJson :: Item -> Value
itemJson it = case it of
  Step s -> tagged "step" (stepFields s)
  KnellItem s -> tagged "knell" (stepFields s)
  Par xs -> tagged "par" ["children" .= map itemJson xs]
  Slot n -> tagged "slot" ["name" .= n]
  Confirm -> tagged "confirm" []
  Commit -> tagged "commit" []
  Preflight gs -> tagged "preflight" ["guards" .= map guardJson gs]
  Observe probe alias -> tagged "observe" ["probe" .= probe, "alias" .= alias]
  Assert g w l -> tagged "assert" ["guard" .= guardJson g, "window_s" .= fmap seconds w, "on_lapse" .= onLapseText l]
  Repeat form v body -> tagged "repeat" ["form" .= repeatFormJson form, "var" .= v, "body" .= map itemJson body]
  When g w l t e ->
    tagged
      "when"
      [ "guard" .= guardJson g
      , "window_s" .= fmap seconds w
      , "on_lapse" .= onLapseText l
      , "then" .= map itemJson t
      , "else" .= map itemJson e
      ]
  where
    tagged tag fields = object (("item" .= (tag :: Text)) : fields)

repeatFormJson :: RepeatForm -> Value
repeatFormJson f = case f of
  Count n -> one "count" (Number (fromIntegral n))
  Over list cap setValued -> one "over" (object ["list" .= list, "max" .= cap, "set_valued" .= setValued])

stepFields :: StepI -> [(Key.Key, Value)]
stepFields s =
  [ "op" .= opJson (stepOp s)
  , "direction" .= (case stepDirection s of Forward -> "forward" :: Text; Inverse -> "inverse")
  , "gate" .= fmap gateJson (stepGate s)
  , "window_s" .= fmap seconds (stepWindow s)
  , "on_lapse" .= onLapseText (stepOnLapse s)
  , "force" .= map forceJson (stepForce s)
  , "alias" .= stepAlias s
  , "args" .= stepArgs s
  ]

forceJson :: ForceName -> Value
forceJson f = case f of
  ForceGuard n -> one "guard" (String n)
  ForceDrift -> String "drift"
  ForceUnknown -> String "unknown"

opJson :: Op -> Value
opJson o =
  object
    [ "id" .= opId o
    , "footprint" .= map footprintJson (opFootprint o)
    , "pre" .= map guardJson (opPre o)
    , "undo" .= undoJson (opUndo o)
    , "post" .= map guardJson (opPost o)
    , "undo_locus" .= (case opUndoLocus o of UndoTarget -> "target" :: Text; UndoController -> "controller"; UndoNone -> "none")
    , "refusal" .= refusalJson (opRefusal o)
    , "drift" .= fmap (\d -> case d of Clobber -> "clobber" :: Text; Defer -> "defer") (opDrift o)
    , "reach" .= opReach o
    , "outputs" .= [object ["name" .= outputName x, "secret" .= outputSecret x] | x <- opOutputs o]
    , "exclusivity" .= opExclusivity o
    , "locus" .= locusJson (opLocus o)
    , "has_suspend" .= opHasSuspend o
    , "handoff_done" .= opHandoffDone o
    , "undo_closed" .= opUndoClosed o
    , "undo_idempotent" .= opUndoIdempotent o
    , "undo_one_line" .= opUndoOneLine o
    ]

footprintJson :: FootprintEntry -> Value
footprintJson e =
  object
    [ "kind" .= kindText (fpKind e)
    , "shape" .= fpShape e
    , "instance" .= fpInstance e
    , "anchor" .= fpAnchor e
    ]

kindText :: Kind -> Text
kindText k = case k of
  Owned -> "owned"
  Region -> "region"
  Modified -> "modified"
  Derived -> "derived"
  AppendOnly -> "append_only"
  Held -> "held"

undoJson :: Undo -> Value
undoJson u = case u of
  Restore -> String "restore"
  Computed pre -> one "computed" (Array (V.fromList (map String pre)))
  Compensate pre -> one "compensate" (Array (V.fromList (map String pre)))
  NoUndo -> String "none"

refusalJson :: Refusal -> Value
refusalJson r = case r of
  Revert -> String "revert"
  Hold via -> one "hold" (object ["via" .= via])
  Knell g c a -> one "knell" (object ["guard" .= fmap guardJson g, "cost" .= costJson c, "ack" .= ackJson a])

costJson :: Cost -> Value
costJson c = case c of
  CostProbe probe -> one "probe" (String probe)
  CostNone reason -> one "none" (String reason)

ackJson :: Ack -> Value
ackJson a = case a of
  AckGate g -> one "gate" (gateJson g)
  AckNone reason -> one "none" (String reason)

locusJson :: Locus -> Value
locusJson l = case l of
  Controller -> String "controller"
  Target -> String "target"
  HostLocus (StaticHost h) -> one "host" (one "static" (String h))
  HostLocus (BoundHost b) -> one "host" (one "bound" (String b))

-- | A one-key object: how a data-carrying constructor is spelled.
one :: Key.Key -> Value -> Value
one k v = object [k .= v]
