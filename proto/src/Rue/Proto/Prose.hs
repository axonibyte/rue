{-# LANGUAGE OverloadedStrings #-}
-- | The verdict prose, docs/ROADMAP.md Appendix A: one clause per field
-- group, in this order: intent, reversibility, hold, point of no return,
-- gate, step gates, backstop, conditionals, controller-only undos, hosts
-- touched, dispatch, may-conflicts, unresolved bindings. A refused verdict
-- leads with its diagnostics instead.
module Rue.Proto.Prose
  ( prose
  ) where

import Data.Maybe (catMaybes, mapMaybe)
import Data.Text (Text)
import qualified Data.Text as T
import Rue.Proto.Diagnostics (codeText)
import Rue.Proto.Intent (Intent (..))
import Data.Aeson (Value (..))
import Rue.Proto.Model (renderDuration)
import Rue.Proto.Verdict

prose :: Verdict -> Text
prose v = case vStatus v of
  RefusedStatus ->
    vPlan v <> " on " <> vHost v <> ": refused; "
      <> T.intercalate "; " (map diag (vDiagnostics v))
      <> ".\n"
  Ok ->
    vPlan v <> " on " <> vHost v <> ": "
      <> T.intercalate "; " (concat clauses)
      <> ".\n"
  where
    diag d = codeText (diagCode d) <> maybe "" (\n -> " at step " <> num n) (diagStep d) <> ": " <> diagMessage d
    clauses =
      [ intentClause
      , [reversibility]
      , map holdClause (vHoldsAt v)
      , maybe [] (pure . knell) (vPointOfNoReturn v)
      , maybe [] (pure . gateClause) (vGate v)
      , mapMaybe stepGate (vSteps v)
      , maybe [] (pure . backstopClause) (vBackstop v)
      , conditionals
      , controllerOnly
      , hosts
      , deferred
      , [dispatch]
      , map mayConflict (vMayConflicts v)
      , unresolved
      ]
    num = T.pack . show
    intentClause = case vIntent v of
      Just Temporary -> ["temporary; reverts at wane " <> maybe "?" renderDuration (vWane v) <> induced]
      Just Permanent ->
        [ "permanent; commits at step " <> maybe "?" num (vCommitStep v)
            <> heldIndef
            <> (if vFiresByConstruction v then ", undo fires by construction" else "")
            <> induced
        ]
      Nothing -> ["intent undetermined"]
    heldIndef = case vHeldIndefinitely v of
      [] -> ""
      ns -> ", held indefinitely at step " <> steps ns <> " until an operator acts"
    induced = case vInducedDefer v of
      [] -> ""
      ns -> ", revert can be induced to defer at step " <> steps ns
    steps = T.intercalate ", " . map num
    total = length (vSteps v)
    reversibility
      | vReversibleThrough v == 0 = "not reversible past step 0"
      | vReversibleThrough v >= total && vPointOfNoReturn v == Nothing = "fully reversible (" <> num total <> " steps)"
      | otherwise = "reversible through step " <> num (vReversibleThrough v)
    -- Phase 0 finding: Appendix A has one hold clause, "(human required)";
    -- section 8.2 says a hold under mode: :auto holds "until resume, recant
    -- or commit", with no human in the loop, and the two cannot both be true.
    holdClause n
      | vMode v == "auto" = "step " <> num n <> " holds on refusal (until resume, recant or commit)"
      | otherwise = "step " <> num n <> " holds on refusal (human required)"
    knell p =
      "step " <> num (ponrStep p) <> " is a point of no return"
        <> maybe "" (", guard " <>) (ponrGuard p)
        <> ", cost " <> ponrCost p
        <> ", acknowledged by " <> ponrAck p
        <> maybe "" (\(f, t) -> "; step " <> num f <> " reversible back to step " <> num t) (vReversibleBackTo v)
    gateClause g
      | not (gvSatisfiable g) = "gate unsatisfiable"
      | gvZeroHumanPath g = "gate satisfiable with no human (allowed)"
      | otherwise = "gate satisfiable; minimum " <> maybe "?" num (gvMinDistinctHumans g) <> " distinct humans"
    stepGate s = case svGate s of
      Nothing -> Nothing
      Just g ->
        Just
          ( "step " <> num (svN s) <> " gated by " <> sgExpr g
              <> maybe "" (\d -> ", satisfiable by wait alone at +" <> renderDuration d) (sgWaitAloneAt g)
          )
    backstopClause b =
      "expiry backstop (" <> T.intercalate ", " (map triggerText (bvTriggers b)) <> ") covers "
        <> range (bvCovers b) <> " on the target"
        <> maybe "" (\n -> ", installed before step " <> num n) (bvInstalledBefore b)
        <> maybe "" (\n -> ", armed before step " <> num n) (bvArmedBefore b)
        <> maybe "" (\n -> ", armed after step " <> num n) (bvArmedAfter b)
        <> (case bvLateArmingWindow b of
              [] -> ""
              w -> ", engine-only for " <> range w <> " until armed")
        <> ", fires within ~" <> renderDuration (bvGranularity b) <> " after the deadline"
        <> (if bvSelfEnforced b then ", self-enforced on " <> vHost v else "")
        <> (case bvDriftPolicy b of
              [] -> ""
              ps -> ", drift: " <> T.intercalate ", " [num n <> " " <> p | (n, p) <- ps])
        <> ", snapshots on target (cap " <> num (bvSnapshotCap b) <> ")"
    triggerText t = case t of
      String s -> s
      other -> T.pack (show other)
    range ns = case ns of
      [] -> "no steps"
      [n] -> "step " <> num n
      _ -> "steps " <> num (minimum ns) <> "\x2013" <> num (maximum ns)
    conditionals =
      [ "step " <> num n <> " reverts unaided unless " <> c <> "; then deferred"
      | Just s <- [vBackstop v]
      , (n, c) <- bvConditional s
      ]
    controllerOnly = case vControllerOnlyUndos v of
      [] -> []
      [n] -> ["step " <> num n <> " reverts only while the engine lives"]
      ns -> ["steps " <> steps ns <> " revert only while the engine lives"]
    hosts =
      catMaybes
        [ case hs of
            [HostTouched h d] | h == vHost v && d == "target" -> Nothing
            [HostTouched "controller" _] -> Nothing
            _ -> Just ("step " <> num n <> " touches " <> T.intercalate ", " (map hostText hs))
        | (n, hs) <- vHostsTouched v
        ]
    hostText h = case h of
      HostTouched hn "controller" -> hn <> " (no instance directory; markers on controller)"
      HostTouched hn _ -> hn
      HostUnresolved _ -> "a host bound at runtime"
    deferred = case vDeferred v of
      [] -> []
      ns -> ["step " <> steps ns <> " deferred (handoff printed)"]
    dispatch = "clause dispatch from " <> dispatchSource (vDispatch v)
    mayConflict m =
      "may-conflict between steps " <> num (mcEarlier m) <> " and " <> num (mcLater m) <> " on " <> mcFact m
        <> (if mcRefused m then " (strict: refused)" else " (warn)")
    unresolved = case vUnresolvedBindings v of
      [] -> []
      bs -> ["unresolved binding(s): " <> T.intercalate ", " bs]
