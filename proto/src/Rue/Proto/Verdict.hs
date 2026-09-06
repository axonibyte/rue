{-# LANGUAGE OverloadedStrings #-}
-- | The verdict's structured form, docs/ROADMAP.md section 5.8, and its
-- canonical JSON. The prose is Rue.Proto.Prose; @explain@ is
-- Rue.Proto.Explain. Every "the verdict says" in the roadmap names a field
-- here; a new clause without a field is a schema bump.
module Rue.Proto.Verdict
  ( Verdict (..)
  , Status (..)
  , Diagnostic (..)
  , PointOfNoReturn (..)
  , GateVerdict (..)
  , BackstopVerdict (..)
  , HostTouched (..)
  , StepVerdict (..)
  , StepGateVerdict (..)
  , KnellVerdict (..)
  , Dispatch (..)
  , MayConflict (..)
  , verdictVersion
  , toJson
  ) where

import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson.Key as Key
import Data.Text (Text)
import qualified Data.Text as T
import qualified Data.Vector as V
import Rue.Proto.Diagnostics (Code, codeText)
import Rue.Proto.Intent (Intent (..))
import Rue.Proto.Model (Duration (..))

verdictVersion :: Int
verdictVersion = 1

data Status = Ok | RefusedStatus
  deriving (Eq, Show)

data Diagnostic = Diagnostic
  { diagCode :: Code
  , diagStep :: Maybe Int
  , diagMessage :: Text
  }
  deriving (Eq, Show)

data PointOfNoReturn = PointOfNoReturn
  { ponrStep :: Int
  , ponrGuard :: Maybe Text
  , ponrCost :: Text
  , ponrAck :: Text
  , ponrGate :: Maybe Text
  }
  deriving (Eq, Show)

data GateVerdict = GateVerdict
  { gvSatisfiable :: Bool
  , gvMinDistinctHumans :: Maybe Int
  , gvZeroHumanPath :: Bool
  , gvWindow :: Maybe Duration
  , gvWaitAloneAt :: Maybe Duration
  }
  deriving (Eq, Show)

data BackstopVerdict = BackstopVerdict
  { bvTriggers :: [Value]
  , bvCovers :: [Int]
  , bvInstalledBefore :: Maybe Int
  , bvArmedBefore :: Maybe Int -- ^ armed before this step, or
  , bvArmedAfter :: Maybe Int -- ^ armed after this step (late arming)
  , bvLateArmingWindow :: [Int]
  , bvScheduler :: Text
  , bvGranularity :: Duration
  , bvSelfEnforced :: Bool
  , bvDriftPolicy :: [(Int, Text)]
  , bvSnapshotLocation :: Text
  , bvSnapshotCap :: Int
  , bvConditional :: [(Int, Text)]
  }
  deriving (Eq, Show)

data HostTouched
  = HostTouched Text Text -- ^ host, directory (@target@ or @controller@)
  | HostUnresolved Text -- ^ where the binding comes from
  deriving (Eq, Show)

data Dispatch = Dispatch {dispatchSource :: Text, dispatchHostContractHash :: Text}
  deriving (Eq, Show)

data MayConflict = MayConflict {mcEarlier :: Int, mcLater :: Int, mcFact :: Text, mcRefused :: Bool}
  deriving (Eq, Show)

data StepGateVerdict = StepGateVerdict
  { sgExpr :: Text
  , sgWindow :: Maybe Duration
  , sgWaitAloneAt :: Maybe Duration
  }
  deriving (Eq, Show)

data KnellVerdict = KnellVerdict
  { kvGuard :: Maybe Text
  , kvCost :: Text
  , kvAck :: Text
  }
  deriving (Eq, Show)

data StepVerdict = StepVerdict
  { svN :: Int
  , svOp :: Text
  , svLocus :: Value
  , svUndo :: Maybe Text
  , svUndoLocus :: Text
  , svRefusal :: Text
  , svDrift :: Maybe Text
  , svGate :: Maybe StepGateVerdict
  , svKnell :: Maybe KnellVerdict
  , svConditional :: Maybe Text
  , svFootprint :: [Text]
  }
  deriving (Eq, Show)

data Verdict = Verdict
  { vPlan :: Text
  , vHost :: Text
  , vStatus :: Status
  , vIntent :: Maybe Intent
  , vRehearsal :: Bool
  , vCommitStep :: Maybe Int
  , vFiresByConstruction :: Bool
  , vWane :: Maybe Duration
  , vReversibleThrough :: Int
  , vHoldsAt :: [Int]
  , vPointOfNoReturn :: Maybe PointOfNoReturn
  , vReversibleBackTo :: Maybe (Int, Int)
  , vGate :: Maybe GateVerdict
  , vBackstop :: Maybe BackstopVerdict
  , vControllerOnlyUndos :: [Int]
  , vHeldIndefinitely :: [Int]
  , vInducedDefer :: [Int]
  , vHostsTouched :: [(Int, [HostTouched])]
  , vDeferred :: [Int]
  , vDispatch :: Dispatch
  , vMayConflicts :: [MayConflict]
  , vUnresolvedBindings :: [Text]
  , vDiagnostics :: [Diagnostic]
  , vSteps :: [StepVerdict]
  }
  deriving (Eq, Show)

-- ---------------------------------------------------------------------------
-- JSON

stepKey :: Int -> Key.Key
stepKey = Key.fromText . T.pack . show

ints :: [Int] -> Value
ints = Array . V.fromList . map (Number . fromIntegral)

dur :: Maybe Duration -> Value
dur = maybe Null (\(Duration s) -> Number (fromIntegral s))

txt :: Maybe Text -> Value
txt = maybe Null String

toJson :: Verdict -> Value
toJson v =
  object
    [ "verdict_version" .= verdictVersion
    , "plan" .= vPlan v
    , "host" .= vHost v
    , "status" .= (case vStatus v of Ok -> "ok" :: Text; RefusedStatus -> "refused")
    , "intent" .= maybe Null (\i -> String (case i of Temporary -> "temporary"; Permanent -> "permanent")) (vIntent v)
    , "rehearsal" .= vRehearsal v
    , "commit_step" .= maybe Null (Number . fromIntegral) (vCommitStep v)
    , "fires_by_construction" .= vFiresByConstruction v
    , "wane_s" .= dur (vWane v)
    , "reversible_through" .= vReversibleThrough v
    , "holds_at" .= ints (vHoldsAt v)
    , "point_of_no_return" .= maybe Null ponr (vPointOfNoReturn v)
    , "reversible_back_to" .= maybe Null (\(f, t) -> object ["from" .= f, "to" .= t]) (vReversibleBackTo v)
    , "gate" .= maybe Null gate (vGate v)
    , "backstop" .= maybe Null backstop (vBackstop v)
    , "controller_only_undos" .= ints (vControllerOnlyUndos v)
    , "held_indefinitely" .= ints (vHeldIndefinitely v)
    , "induced_defer" .= ints (vInducedDefer v)
    , "hosts_touched" .= object [stepKey n .= Array (V.fromList (map host hs)) | (n, hs) <- vHostsTouched v]
    , "deferred" .= ints (vDeferred v)
    , "dispatch" .= object ["source" .= dispatchSource (vDispatch v), "host_contract_hash" .= dispatchHostContractHash (vDispatch v)]
    , "may_conflicts" .= Array (V.fromList (map mayConflict (vMayConflicts v)))
    , "unresolved_bindings" .= vUnresolvedBindings v
    , "diagnostics" .= Array (V.fromList (map diagnostic (vDiagnostics v)))
    , "steps" .= Array (V.fromList (map stepV (vSteps v)))
    ]
  where
    ponr p =
      object
        [ "step" .= ponrStep p
        , "guard" .= txt (ponrGuard p)
        , "cost" .= ponrCost p
        , "ack" .= ponrAck p
        , "gate" .= txt (ponrGate p)
        ]
    gate g =
      object
        [ "satisfiable" .= gvSatisfiable g
        , "min_distinct_humans" .= maybe Null (Number . fromIntegral) (gvMinDistinctHumans g)
        , "zero_human_path" .= gvZeroHumanPath g
        , "window_s" .= dur (gvWindow g)
        , "wait_alone_at_s" .= dur (gvWaitAloneAt g)
        ]
    backstop b =
      object
        [ "triggers" .= bvTriggers b
        , "covers" .= ints (bvCovers b)
        , "locus" .= ("target" :: Text)
        , "installed_before" .= maybe Null (Number . fromIntegral) (bvInstalledBefore b)
        , "armed_before" .= maybe Null (Number . fromIntegral) (bvArmedBefore b)
        , "armed_after" .= maybe Null (Number . fromIntegral) (bvArmedAfter b)
        , "late_arming_window" .= ints (bvLateArmingWindow b)
        , "scheduler" .= bvScheduler b
        , "granularity_s" .= seconds (bvGranularity b)
        , "self_enforced" .= bvSelfEnforced b
        , "drift_policy" .= object [stepKey n .= p | (n, p) <- bvDriftPolicy b]
        , "snapshots" .= object ["location" .= bvSnapshotLocation b, "cap_bytes" .= bvSnapshotCap b]
        , "conditional" .= Array (V.fromList [object ["step" .= n, "on" .= c] | (n, c) <- bvConditional b])
        ]
    host h = case h of
      HostTouched hn d -> object ["host" .= hn, "directory" .= d]
      HostUnresolved src -> object ["unresolved" .= src]
    mayConflict m =
      object ["earlier" .= mcEarlier m, "later" .= mcLater m, "fact" .= mcFact m, "refused" .= mcRefused m]
    diagnostic d =
      object ["code" .= codeText (diagCode d), "step" .= maybe Null (Number . fromIntegral) (diagStep d), "message" .= diagMessage d]
    stepV s =
      object
        [ "n" .= svN s
        , "op" .= svOp s
        , "locus" .= svLocus s
        , "undo" .= txt (svUndo s)
        , "undo_locus" .= svUndoLocus s
        , "refusal" .= svRefusal s
        , "drift" .= txt (svDrift s)
        , "gate" .= maybe Null (\g -> object ["expr" .= sgExpr g, "step_digest" .= True, "window_s" .= dur (sgWindow g), "wait_alone_at_s" .= dur (sgWaitAloneAt g)]) (svGate s)
        , "knell" .= maybe Null (\k -> object ["guard" .= txt (kvGuard k), "cost" .= kvCost k, "ack" .= kvAck k]) (svKnell s)
        , "conditional" .= txt (svConditional s)
        , "footprint" .= svFootprint s
        ]
