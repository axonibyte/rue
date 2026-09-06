{-# LANGUAGE OverloadedStrings #-}
-- | @check :: Site -> Requester -> Plan -> Verdict@, docs/ROADMAP.md
-- sections 5.3 to 5.7 and 5.11, assembled.
--
-- The requester is an input: the requester exclusion (E0508) is a check-time
-- rule, and an offline check has no session to read it from. That is one of
-- Phase 0's recorded findings for section 5.11.
module Rue.Proto.Check
  ( Requester
  , check
  , effectiveDrift
  , deferredSteps
  , triggerPretty
  ) where

import Data.Aeson (Value (..))
import Data.List (nub, sort)
import Data.Maybe (isJust, isNothing, mapMaybe)
import Data.Text (Text)
import qualified Data.Text as T
import Rue.Proto.Algebra (numbered)
import Rue.Proto.Backstop
import Rue.Proto.Diagnostics (Code (..))
import qualified Rue.Proto.Gates as G
import Rue.Proto.Intent
import Rue.Proto.Interference
import Rue.Proto.Model
import Rue.Proto.Verdict

type Requester = Text

-- | The drift policy in force: the op's, else its footprint kind's default.
effectiveDrift :: Op -> Maybe Drift
effectiveDrift o = case opDrift o of
  Just d -> Just d
  Nothing -> case mapMaybe (defaultDrift . fpKind) (opFootprint o) of
    d : _ -> Just d
    [] -> Nothing

opOf :: Item -> Maybe Op
opOf it = case it of
  Step s -> Just (stepOp s)
  KnellItem s -> Just (stepOp s)
  _ -> Nothing

stepIOf :: Item -> Maybe StepI
stepIOf it = case it of
  Step s -> Just s
  KnellItem s -> Just s
  _ -> Nothing

-- | The host a step acts on, resolved where it can be.
stepHost :: Plan -> Op -> Either Text Host
stepHost p o = case opLocus o of
  Controller -> Right (planOwner p)
  Target -> Right (planOwner p)
  HostLocus (StaticHost h) -> Right h
  HostLocus (BoundHost b) -> Left b

hostRecord :: Site -> Host -> Maybe HostRecord
hostRecord site h = case [r | r <- siteHosts site, hrName r == h] of
  r : _ -> Just r
  [] -> Nothing

-- | A step is deferred when its host is one the running engine cannot act
-- on: a static host none of the site's transports reach, or a host bound at
-- runtime. Phase 0 finding: this rule is what section 5.12 needs to say.
deferredSteps :: Site -> Plan -> [Int]
deferredSteps site p =
  [ n
  | (n, it) <- numbered (planBody p)
  , Just o <- [opOf it]
  , case stepHost p o of
      Left _ -> True
      Right h -> h /= planOwner p && not (reachable h)
  ]
  where
    reachable h = case hostRecord site h of
      Nothing -> False
      Just r -> any (`elem` siteTransports site) (hrReach r)

-- | Trigger text as the surface spells it.
triggerPretty :: Trigger -> Text
triggerPretty t = case t of
  After d -> "after " <> renderDuration d
  UnlessConfirmed d -> "unless confirmed within " <> renderDuration d
  UnlessHeartbeat d Nothing -> "unless heartbeat within " <> renderDuration d
  UnlessHeartbeat d (Just i) -> "unless heartbeat within " <> renderDuration d <> " every " <> renderDuration i

check :: Site -> Requester -> Plan -> Verdict
check site requester p =
  Verdict
    { vPlan = planId p
    , vHost = planOwner p
    , vStatus = if null diagnostics then Ok else RefusedStatus
    , vIntent = intent
    , vRehearsal = False
    , vCommitStep = if intent == Just Permanent then commitStep body else Nothing
    , vFiresByConstruction = planFiresByConstruction p
    , vWane = effectiveWane p
    , vReversibleThrough = reversibleThrough
    , vHoldsAt = holdsAt
    , vPointOfNoReturn = ponr
    , vReversibleBackTo = backTo
    , vGate = gateVerdict
    , vBackstop = backstopVerdict
    , vControllerOnlyUndos = controllerOnly
    , vHeldIndefinitely = heldIndefinitely
    , vInducedDefer = inducedDefer
    , vHostsTouched = hostsTouched
    , vDeferred = deferred
    , vDispatch = Dispatch "inventory" "-"
    , vMayConflicts = mayConflictVerdicts
    , vUnresolvedBindings = []
    , vDiagnostics = diagnostics
    , vSteps = map stepVerdict steps
    }
  where
    body = planBody p
    steps = numbered body
    stepOps = [(n, o) | (n, it) <- steps, Just o <- [opOf it]]
    intent = inferIntent p
    deferred = deferredSteps site p
    firstKnell = case [n | (n, KnellItem _) <- steps] of
      n : _ -> Just n
      [] -> Nothing
    lastStep = length steps

    -- Reversibility: through the last mutating step before the first knell
    -- when the interference query finds no conflict; otherwise refused
    -- (E0301), and the verdict says step 0. Items that change nothing
    -- (confirm, commit, observe, assert, preflight) do not extend it.
    conflictList = conflicts body
    reversibleThrough
      | not (null conflictList) = 0
      | otherwise = case [n | (n, _) <- stepOps, maybe True (n <) firstKnell] of
          [] -> 0
          ns -> maximum ns
    -- Holding: the first :hold step in each knell segment. Phase 0 finding:
    -- section 5.5 says "at or before the first knell"; T2 holds after it.
    segments = splitSegments (map fst steps) (sort [n | (n, KnellItem _) <- steps])
    holdsAt = mapMaybe firstHold segments
    firstHold seg = case [n | n <- seg, Just o <- [lookup n stepOps], isHold (opRefusal o)] of
      n : _ -> Just n
      [] -> Nothing
    isHold r = case r of
      Hold _ -> True
      _ -> False
    ponr = case firstKnell of
      Nothing -> Nothing
      Just n -> case lookup n stepOps of
        Just o@Op {opRefusal = Knell g c a} ->
          Just
            PointOfNoReturn
              { ponrStep = n
              , ponrGuard = guardName <$> g
              , ponrCost = costText c
              , ponrAck = ackText a
              , ponrGate = case [s | (m, it) <- steps, m == n, Just s <- [stepIOf it]] of
                  s : _ -> G.renderGate <$> stepGate s
                  [] -> Nothing
              }
            <* Just o
        _ -> Nothing
    backTo = case firstKnell of
      Just n | n < lastStep -> Just (lastStep, n)
      _ -> Nothing
    costText c = case c of
      CostProbe probe -> probe
      CostNone _ -> "none"
    ackText a = case a of
      AckGate g -> G.renderGate g
      AckNone _ -> "none"

    -- Gates.
    auths = siteAuthenticators site
    gateVerdict = case planGate p of
      Nothing -> Nothing
      Just pg ->
        let r = G.report auths (gateExpr pg)
         in Just
              GateVerdict
                { gvSatisfiable = G.satisfiable r
                , gvMinDistinctHumans = G.minDistinctHumans r
                , gvZeroHumanPath = G.zeroHumanPath r
                , gvWindow = gateWindow pg
                , gvWaitAloneAt = G.waitAloneAt r
                }

    -- Backstop coverage.
    cov = coverage p
    backstopVerdict = case (planBackstop p, cov) of
      (Just b, Just c) ->
        let ArmBefore a = armBefore b
            firstCovered = installedBefore c
            armedBeforeV = if maybe True (a <=) firstCovered then Just a else Nothing
            armedAfterV = if isNothing armedBeforeV then Just (a - 1) else Nothing
         in Just
              BackstopVerdict
                { bvTriggers = map (\t -> String (triggerPretty t)) (triggers b)
                , bvCovers = covered c
                , bvInstalledBefore = firstCovered
                , bvArmedBefore = armedBeforeV
                , bvArmedAfter = armedAfterV
                , bvLateArmingWindow = lateArmingWindow c
                , bvScheduler = "cron"
                , bvGranularity = Duration 60
                , bvSelfEnforced = True
                , bvDriftPolicy = [(n, driftName o) | (n, o) <- stepOps, n `elem` covered c]
                , bvSnapshotLocation = "target"
                , bvSnapshotCap = 1048576
                , bvConditional = conditionalSteps
                }
      _ -> Nothing
    driftName o = case effectiveDrift o of
      Just Clobber -> "clobber"
      Just Defer -> "defer"
      Nothing -> "n/a"
    -- A region step's damaged-marker fallback is conditional on no other
    -- instance holding a region on the fact (sections 5.2, D-053).
    conditionalSteps =
      [ (n, "foreign region in " <> fpShape e)
      | (n, o) <- stepOps
      , effectiveDrift o == Just Clobber
      , e <- opFootprint o
      , fpKind e == Region
      ]
    controllerOnly = [n | (n, o) <- stepOps, opUndoLocus o == UndoController]
    heldIndefinitely
      | intent == Just Permanent = holdsAt <> deferred
      | otherwise = []
    inducedDefer
      | planMode p == Auto = [n | (n, o) <- stepOps, effectiveDrift o == Just Defer]
      | otherwise = []
    hostsTouched =
      [ ( n
        , case stepHost p o of
            Left b -> [HostUnresolved ("bound from " <> b)]
            Right h -> [HostTouched h (if maybe False hrFilesystem (hostRecord site h) then "target" else "controller")]
        )
      | (n, o) <- stepOps
      ]

    -- May-conflicts.
    mays = mayConflicts body
    mayConflictVerdicts = [MayConflict a b (factText f) (planStrictness p == Strict) | Conflict a b f <- mays]
    factText (Fact s a) = s <> maybe "" (\x -> " (anchor " <> x <> ")") a

    -- Diagnostics, in code order.
    diagnostics = sortOnCode (concat [opDiagnostics, planDiagnostics, interferenceDiagnostics, backstopDiagnostics, intentDiagnostics, waitDiagnostics, gateDiagnostics])
    sortOnCode ds = [x | c <- nub (sort (map diagCode ds)), x <- ds, diagCode x == c]
    d code n msg = Diagnostic code n msg

    opDiagnostics =
      concat
        [ [d E0201 (Just n) ("op " <> opId o <> ": undo and knell disagree") | (opUndo o == NoUndo) /= isKnell (opRefusal o)]
            <> [d E0202 (Just n) ("op " <> opId o <> ": :target undo is not closed over target-local commands and facts") | opUndoLocus o == UndoTarget, not (opUndoClosed o)]
            <> [d E0203 (Just n) ("op " <> opId o <> ": undo_locus :none with an undo body") | opUndoLocus o == UndoNone, opUndo o /= NoUndo]
            <> [d E0205 (Just n) ("op " <> opId o <> ": held footprint without suspend/reestablish") | any ((== Held) . fpKind) (opFootprint o), not (opHasSuspend o)]
            <> [d E0207 (Just n) ("op " <> opId o <> ": computed or compensating undo without undo_pre") | emptyPre (opUndo o)]
            <> [d E0208 (Just n) ("op " <> opId o <> ": undo not provably idempotent") | not (opUndoIdempotent o)]
            <> [d E0407 (Just n) ("op " <> opId o <> ": :target undo on a host with no run-capable executor") | opUndoLocus o == UndoTarget, noFilesystem o]
            <> [d E0410 (Just n) ("op " <> opId o <> ": reach op whose undo is drift: :defer") | not (null (opReach o)), effectiveDrift o == Just Defer]
        | (n, o) <- stepOps
        ]
    isKnell r = case r of
      Knell {} -> True
      _ -> False
    emptyPre u = case u of
      Computed [] -> True
      Compensate [] -> True
      _ -> False
    noFilesystem o = case stepHost p o of
      Right h -> maybe True (not . hrFilesystem) (hostRecord site h)
      Left _ -> False

    planDiagnostics =
      [d E0404 Nothing "mode: :auto plan contains force:" | planMode p == Auto, any hasForce steps]
        <> [d E0403 Nothing ("backstop scheduler not present on " <> planOwner p) | isJust (planBackstop p), planOwner p `notElem` siteSchedulerPresent site]
    hasForce (_, it) = case stepIOf it of
      Just s -> not (null (stepForce s))
      Nothing -> False

    interferenceDiagnostics =
      [d E0301 (Just b) ("steps " <> tshow a <> " and " <> tshow b <> " both write " <> factText f <> " and step " <> tshow a <> "'s undo needs it") | Conflict a b f <- conflictList]
        <> [d E0302 (Just b) ("may-conflict between steps " <> tshow a <> " and " <> tshow b <> " on " <> factText f) | planStrictness p == Strict, Conflict a b f <- mays]
        <> [d E0303 (Just b) ("par children at steps " <> tshow a <> " and " <> tshow b <> " are not umbra-disjoint") | (a, b) <- fst (parViolations body)]
        <> [d E0304 (Just n) "reach op inside par" | n <- snd (parViolations body)]
        <> [d E0305 (Just b) ("anchor declared twice on " <> factText f <> " (steps " <> tshow a <> ", " <> tshow b <> ")") | Conflict a b f <- anchorDuplicates body]
    tshow = T.pack . show

    backstopDiagnostics =
      [d E0401 (Just n) "reach op without a preceding armed :target backstop" | n <- reachViolations p]
        <> [d E0405 Nothing ("heartbeat interval exceeds a third of its deadline: " <> triggerPretty t) | t <- heartbeatViolations p]
        <> case intent of
          Just i ->
            concat
              [ case tv of
                  AfterNotWane -> [d E0503 Nothing "temporary plan's backstop after: differs from its wane"]
                  TemporaryConfirmed -> [d E0503 Nothing "temporary plan's backstop is unless_confirmed without fires_by_construction"]
                  PermanentAfter -> [d E0504 Nothing "permanent plan's backstop expires on a timer"]
                  NoConfirmOrCommitPath k -> [d E0504 Nothing (tshow k <> " path(s) reach neither confirm() nor commit()")]
              | tv <- triggerViolations i p
              ]
          Nothing -> []

    intentDiagnostics =
      [d E0501 Nothing "intent undeterminable: both wane and commit(), or neither" | isNothing intent]
        <> [d E0502 Nothing "commit() is not the last item on its path" | commitNotLast body]
        <> [d E0505 Nothing (tshow k <> " non-refusing path(s) never reach commit()") | intent == Just Permanent, let k = pathsWithoutCommit body, k > 0]

    -- Waits and bounds (section 5.9, rules 2 and 3).
    canWait = not (null waitingSteps)
    waitingSteps =
      [ n
      | (n, it) <- steps
      , case it of
          Assert {} -> True
          When {} -> True
          _ -> case stepIOf it of
            Just s -> isJust (stepGate s) || knellWaits (opRefusal (stepOp s))
            Nothing -> False
      ]
    knellWaits r = case r of
      Knell g _ (AckGate _) -> True || isJust g
      Knell (Just g) _ _ -> guardValue g == Unknown
      _ -> False
    canHold = not (null holdsAt) || any lapseHold steps
    lapseHold (_, it) = case it of
      Assert _ _ LapseHold -> True
      When _ _ LapseHold _ _ -> True
      _ -> maybe False ((== LapseHold) . stepOnLapse) (stepIOf it)
    unboundedWait n = case lookup n [(m, it) | (m, it) <- steps] of
      Just (Assert _ w _) -> isNothing w && isNothing (siteMaxWait site)
      Just (When _ w _ _ _) -> isNothing w && isNothing (siteMaxWait site)
      Just it -> maybe False (\s -> isNothing (stepWindow s) && isNothing (siteMaxWait site)) (stepIOf it)
      Nothing -> False
    waitDiagnostics = case intent of
      Just Temporary
        | isNothing (effectiveWane p) && (canWait || canHold || not (null deferred)) ->
            [d E0506 Nothing "temporary plan can reach Waiting, Held or Deferred without wane"]
        | otherwise -> pendingUnbounded
      Just Permanent ->
        [d E0506 (Just n) "permanent plan's wait has neither window: nor a site max_wait" | n <- waitingSteps, unboundedWait n] <> pendingUnbounded
      Nothing -> []
    -- Phase 0 finding: Pending's bound is its approval window; a gated plan
    -- with no window and no max_wait reserves umbras until cancelled.
    pendingUnbounded = case planGate p of
      Just pg | isNothing (gateWindow pg), isNothing (siteMaxWait site) -> [d E0506 Nothing "plan-entry gate has no window: and the site has no max_wait; Pending would be unbounded"]
      _ -> []

    gateDiagnostics =
      concat
        [ gateChecks Nothing (gateExpr pg) (allowZeroHuman pg) | Just pg <- [planGate p]
        ]
        <> concat [gateChecks (Just n) g False | (n, it) <- steps, Just s <- [stepIOf it], Just g <- [stepGate s]]
        <> concat [ackChecks n r | (n, o) <- stepOps, let r = opRefusal o, isKnell r]
    gateChecks n g allowZero =
      [d E0508 n ("gate names unknown authenticator(s): " <> T.intercalate ", " unknown) | let unknown = G.unknownAuthenticators auths g, not (null unknown)]
        <> [d E0508 n "gate is unsatisfiable" | not (G.satisfiable r)]
        <> [d E0508 n ("gate counts the requester " <> requester) | G.countsRequester auths requester g]
        <> [d E0509 n "gate satisfiable with zero human authenticators and no allow_zero_human" | G.zeroHumanPath r, not allowZero || planMode p == Auto]
        <> [d E0507 n "mode: :auto plan has a step gate needing a human" | isJust n, planMode p == Auto, not (G.zeroHumanPath r)]
      where
        r = G.report auths g
    ackChecks n r = case r of
      Knell _ _ (AckGate g) ->
        [d E0507 (Just n) "mode: :auto plan has a knell whose ack: is not :none" | planMode p == Auto]
          <> [d E0508 (Just n) ("ack names unknown authenticator(s): " <> T.intercalate ", " unknown) | let unknown = G.unknownAuthenticators auths g, not (null unknown)]
          <> [d E0508 (Just n) "ack is unsatisfiable" | not (G.satisfiable (G.report auths g))]
      _ -> []

    stepVerdict (n, it) = case opOf it of
      Just o ->
        StepVerdict
          { svN = n
          , svOp = opId o
          , svLocus = case opLocus o of
              Controller -> String "controller"
              Target -> String "target"
              HostLocus (StaticHost h) -> String ("host(" <> h <> ")")
              HostLocus (BoundHost b) -> String ("host bound from " <> b)
          , svUndo = case opUndo o of
              NoUndo -> Nothing
              _ -> Just (opUndoOneLine o)
          , svUndoLocus = case opUndoLocus o of
              UndoTarget -> "target"
              UndoController -> "controller"
              UndoNone -> "none"
          , svRefusal = case opRefusal o of
              Revert -> "revert"
              Hold _ -> "hold"
              Knell {} -> "knell"
          , svDrift = case effectiveDrift o of
              Just Clobber -> Just "clobber"
              Just Defer -> Just "defer"
              Nothing -> Nothing
          , svGate = case stepIOf it >>= stepGate of
              Just g -> Just (StepGateVerdict (G.renderGate g) (stepIOf it >>= stepWindow) (G.waitAloneAt (G.report auths g)))
              Nothing -> Nothing
          , svKnell = case opRefusal o of
              Knell g c a -> Just (KnellVerdict (guardName <$> g) (costText c) (ackText a))
              _ -> Nothing
          , svConditional = lookup n conditionalSteps
          , svFootprint = [kindText (fpKind e) <> ": " <> fpShape e <> maybe "" (\a -> " anchor " <> a) (fpAnchor e) | e <- opFootprint o]
          }
      Nothing ->
        StepVerdict
          { svN = n
          , svOp = itemName it
          , svLocus = String "controller"
          , svUndo = Nothing
          , svUndoLocus = "none"
          , svRefusal = "n/a"
          , svDrift = Nothing
          , svGate = Nothing
          , svKnell = Nothing
          , svConditional = Nothing
          , svFootprint = []
          }
    itemName it = case it of
      Confirm -> "confirm()"
      Commit -> "commit()"
      Preflight _ -> "preflight"
      Observe probe _ -> "observe " <> probe
      Assert g _ _ -> "assert " <> guardName g
      Slot s -> "slot :" <> s
      _ -> "item"
    kindText k = case k of
      Owned -> "owned"
      Region -> "region"
      Modified -> "modified"
      Derived -> "derived"
      AppendOnly -> "append_only"
      Held -> "held"

-- | Split step numbers into segments: before the first knell, and after each
-- knell up to the next.
splitSegments :: [Int] -> [Int] -> [[Int]]
splitSegments ns knells = case knells of
  [] -> [ns]
  k : rest -> takeWhile (< k) ns : splitSegments (filter (> k) ns) rest
