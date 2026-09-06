{-# LANGUAGE OverloadedStrings #-}
-- | @explain@, docs/ROADMAP.md Appendix B: one line per numbered step.
--
-- >  N. <op>[(<args>)]   locus=<L>   refusal=<R>   drift=<D>   undo=<one-line undo or "NO UNDO — knell, cost <C>">   undo_locus=<UL>   [gate=<G>]   [ack=<A>]   [deferred → <handoff cmd>]
--
-- Secrets render as @<secret:label>@; a @region@ op with @drift: :clobber@
-- prints its damaged-marker cost.
module Rue.Proto.Explain
  ( explain
  ) where

import Data.Text (Text)
import qualified Data.Text as T
import Rue.Proto.Algebra (numbered)
import Rue.Proto.Gates (renderGate)
import Rue.Proto.Model

explain :: Plan -> [Int] -> Text
explain p deferredSteps = T.unlines (map line (numbered (planBody p)))
  where
    line (n, it) = T.justifyRight 2 ' ' (T.pack (show n)) <> ". " <> body n it
    body n it = case it of
      Step s -> stepLine n s
      KnellItem s -> stepLine n s
      Confirm -> "confirm()   disarms the unless_confirmed backstop"
      Commit -> "commit()   ends the plan: undo discarded, umbras released, backstops disarmed"
      Preflight gs -> "preflight   guards: " <> T.intercalate ", " (map guardName gs) <> "   (measured before any mutation and again at point of use)"
      Observe probe alias -> "observe " <> probe <> " as " <> alias <> "   no footprint, no undo"
      Assert g _ _ -> "assert " <> guardName g <> "   :no refuses; :unknown waits"
      Slot name -> "slot :" <> name
      -- Containers never appear among the numbered leaves; named for totality.
      Par _ -> "par"
      Repeat _ _ _ -> "repeat"
      When g _ _ _ _ -> "when " <> guardName g
    stepLine n s =
      let o = stepOp s
          args = if null (stepArgs s) then "" else "(" <> T.intercalate ", " (stepArgs s) <> ")"
       in T.intercalate "   " $
            [ opId o <> args
            , "locus=" <> locusText (opLocus o)
            , "refusal=" <> refusalText (opRefusal o)
            , "drift=" <> driftText o
            , "undo=" <> undoText o
            , "undo_locus=" <> undoLocusText (opUndoLocus o)
            ]
              <> maybe [] (\g -> ["gate=" <> renderGate g]) (stepGate s)
              <> ackPart (opRefusal o)
              <> [regionCost o | hasRegionClobber o]
              <> ["deferred \x2192 " <> maybe "(handoff command printed at apply)" ("handoff_done: " <>) (opHandoffDone o) | n `elem` deferredSteps]
    locusText l = case l of
      Controller -> "controller"
      Target -> "target"
      HostLocus (StaticHost h) -> "host(" <> h <> ")"
      HostLocus (BoundHost b) -> "host(" <> b <> ") bound at runtime"
    refusalText r = case r of
      Revert -> "revert"
      Hold Nothing -> "hold"
      Hold (Just via) -> "hold via " <> via
      Knell {} -> "knell"
    driftText o = case effectiveDrift o of
      Just Clobber -> "clobber"
      Just Defer -> "defer"
      Nothing -> "n/a"
    undoText o = case opUndo o of
      NoUndo -> "NO UNDO \x2014 knell, cost " <> costText (opRefusal o)
      _ -> redact (opUndoOneLine o) o
    redact t o = foldr (\out acc -> if outputSecret out then T.replace (outputName out) ("<secret:" <> outputName out <> ">") acc else acc) t (opOutputs o)
    costText r = case r of
      Knell _ (CostProbe probe) _ -> probe
      Knell _ (CostNone reason) _ -> "none (" <> reason <> ")"
      _ -> "none"
    ackPart r = case r of
      Knell _ _ (AckGate g) -> ["ack=" <> renderGate g]
      Knell _ _ (AckNone reason) -> ["ack=none (" <> reason <> ")"]
      _ -> []
    undoLocusText u = case u of
      UndoTarget -> "target"
      UndoController -> "controller"
      UndoNone -> "none"
    hasRegionClobber o = any (\e -> fpKind e == Region) (opFootprint o) && effectiveDrift o == Just Clobber
    regionCost _ = "damaged-marker cost: the whole fact is restored from the do-time snapshot and a stranger's edits outside the region are lost, unless another instance holds a region on it"

effectiveDrift :: Op -> Maybe Drift
effectiveDrift o = case opDrift o of
  Just d -> Just d
  Nothing -> case [k | e <- opFootprint o, let k = fpKind e, defaultDrift k /= Nothing] of
    k : _ -> defaultDrift k
    [] -> Nothing
