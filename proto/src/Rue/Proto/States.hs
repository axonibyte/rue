{-# LANGUAGE OverloadedStrings #-}
-- | The runtime state machine, docs/ROADMAP.md section 5.9, derived from its
-- five class rules rather than drawn:
--
-- 1. Terminal: Closed, Committed.
-- 2. Bounded by wane in a temporary plan: every non-terminal state except
--    DriftHeld and Stuck; Pending's bound is its approval window. Wane
--    elapsing is always Expired then Reverting, never a hold.
-- 3. Unbounded, by declaration: DriftHeld and Stuck in any plan; Held and
--    Deferred in a permanent plan. A permanent plan's Waiting is bounded by
--    its window or the site's max_wait.
-- 4. Refusal during Applying goes to Reverting, unless an earlier applied
--    step has refusal :hold, in which case to Held. A window or max_wait
--    lapse before wane resolves per on_lapse (revert default; hold; always
--    revert under auto).
-- 5. Commit is reached from Applying by the item, from Held or Deferred by
--    the verb; commit, renew and confirm on a plan whose intent does not
--    admit them are R0102.
--
-- The table @rue-proto-states@ prints, and the tier-4 truth-table test, are
-- both generated from 'transition'.
module Rue.Proto.States
  ( State (..)
  , Event (..)
  , Outcome (..)
  , RCode (..)
  , Ctx (..)
  , allStates
  , allEvents
  , terminal
  , transition
  , transitionTable
  , renderTable
  ) where

import Data.Text (Text)
import qualified Data.Text as T
import Rue.Proto.Intent (Intent (..))
import Rue.Proto.Model (Mode (..), OnLapse (..))

data State
  = Unchecked
  | Checked
  | Pending
  | ApprovalExpired
  | Applying
  | Waiting
  | Deferred
  | Held
  | Applied
  | Suspended
  | Expired
  | Reverting
  | Stuck
  | DriftHeld
  | Committed
  | Closed
  deriving (Eq, Ord, Show, Enum, Bounded)

data Event
  = EvCheck
  | EvRequest
  | EvApprove
  | EvApprovalWindowLapses
  | EvCancel
  | EvHostContractChanged
  | EvAllStepsDone
  | EvCommitItem
  | EvRefuse
  | EvWaitAtStep -- ^ a gate, ack or unknown guard at a step
  | EvWaitSatisfied -- ^ satisfied, acked or forced
  | EvDeferAtStep
  | EvHandoffDone
  | EvRecant
  | EvSuspend
  | EvReestablish
  | EvRenew
  | EvConfirm
  | EvCommitVerb
  | EvResume
  | EvBoundLapses -- ^ the step's window or the site's max_wait, before wane
  | EvWaneElapses
  | EvUndoClean
  | EvUndoFailed
  | EvRetry
  | EvDriftOnDefer
  | EvForceDrift
  | EvAbandon
  deriving (Eq, Ord, Show, Enum, Bounded)

-- | Runtime codes from Appendix D that the machine can name.
data RCode
  = R0102 -- ^ verb not admitted by the plan's intent
  | R0103 -- ^ recant on DriftHeld without --force=drift
  deriving (Eq, Ord, Show)

data Outcome
  = To State
  | Stay -- ^ the event is observed and the state is unchanged
  | Refuse RCode -- ^ the event is refused with a code; the state is unchanged
  | NotApplicable -- ^ the event cannot occur in this state
  deriving (Eq, Ord, Show)

-- | What a transition may depend on besides the state and the event.
data Ctx = Ctx
  { ctxIntent :: Intent
  , ctxMode :: Mode
  , ctxEarlierHold :: Bool -- ^ an earlier applied step has refusal :hold
  , ctxOnLapse :: OnLapse
  }
  deriving (Eq, Ord, Show)

allStates :: [State]
allStates = [minBound .. maxBound]

allEvents :: [Event]
allEvents = [minBound .. maxBound]

terminal :: State -> Bool
terminal s = s == Closed || s == Committed

transition :: Ctx -> State -> Event -> Outcome
transition ctx s ev
  | terminal s = NotApplicable -- rule 1
  -- Applied and Suspended are temporary-plan states: a permanent plan goes
  -- from Applying to Committed and never rests as Applied, so nothing can
  -- happen to it there.
  | permanent && (s == Applied || s == Suspended) = NotApplicable
  | ev == EvWaneElapses = wane -- rule 2
  | ev == EvAbandon = if s == Stuck || s == DriftHeld then To Closed else NotApplicable
  | otherwise = case (s, ev) of
      (Unchecked, EvCheck) -> To Checked
      (Checked, EvRequest) -> To Pending
      (Pending, EvApprove) -> To Applying
      (Pending, EvApprovalWindowLapses) -> To ApprovalExpired
      (Pending, EvCancel) -> To Closed
      (Pending, EvHostContractChanged) -> To Closed
      (ApprovalExpired, EvCancel) -> To Closed -- reaped
      (Applying, EvAllStepsDone) -> if temporary then To Applied else NotApplicable
      (Applying, EvCommitItem) -> if permanent then To Committed else Refuse R0102
      (Applying, EvRefuse) -> if ctxEarlierHold ctx then To Held else To Reverting -- rule 4
      (Applying, EvWaitAtStep) -> To Waiting
      (Applying, EvDeferAtStep) -> To Deferred
      (Applying, EvConfirm) -> if permanent then Stay else Refuse R0102
      (Applying, EvRenew) -> if temporary then Stay else Refuse R0102 -- wane is anchored at approval, so renewal is meaningful while applying
      (Waiting, EvWaitSatisfied) -> To Applying
      (Waiting, EvRecant) -> To Reverting
      (Waiting, EvBoundLapses) -> lapse -- rule 4
      (Deferred, EvHandoffDone) -> To Applying
      (Deferred, EvRecant) -> To Reverting
      (Deferred, EvCommitVerb) -> if permanent then To Committed else Refuse R0102 -- rule 5
      (Held, EvResume) -> To Applying
      (Held, EvRecant) -> To Reverting
      (Held, EvCommitVerb) -> if permanent then To Committed else Refuse R0102 -- rule 5
      (Applied, EvRecant) -> To Reverting
      (Applied, EvSuspend) -> To Suspended
      (Applied, EvRenew) -> if temporary then Stay else Refuse R0102
      (Applied, EvConfirm) -> if permanent then Stay else Refuse R0102
      (Suspended, EvReestablish) -> To Applied
      (Suspended, EvRecant) -> To Reverting
      (Expired, EvUndoClean) -> To Closed -- Expired reverts: same exits as Reverting
      (Expired, EvUndoFailed) -> To Stuck
      (Expired, EvDriftOnDefer) -> To DriftHeld
      (Reverting, EvUndoClean) -> To Closed
      (Reverting, EvUndoFailed) -> To Stuck
      (Reverting, EvDriftOnDefer) -> To DriftHeld
      (Stuck, EvRetry) -> To Reverting
      (DriftHeld, EvForceDrift) -> To Reverting
      (DriftHeld, EvRecant) -> Refuse R0103
      _ -> NotApplicable
  where
    temporary = ctxIntent ctx == Temporary
    permanent = ctxIntent ctx == Permanent
    -- Rule 2 and rule 3: wane bounds every non-terminal state of a temporary
    -- plan except DriftHeld and Stuck (unbounded by declaration) and Pending
    -- (bounded by its approval window instead). A permanent plan has no
    -- wane, so the event cannot occur.
    wane
      | permanent = NotApplicable
      | s == DriftHeld || s == Stuck = Stay
      | s == Pending || s == Unchecked || s == Checked || s == ApprovalExpired = NotApplicable
      | s == Expired = Stay
      | otherwise = To Expired
    -- Rule 4: a lapse before wane resolves per on_lapse; always revert under
    -- auto. Held in a permanent plan is unbounded (rule 3), so a lapse into
    -- Held is a hold until an operator acts.
    lapse
      | ctxMode ctx == Auto = To Reverting
      | ctxOnLapse ctx == LapseHold = To Held
      | otherwise = To Reverting

-- | Every (context, state, event) with its outcome, over both intents, both
-- modes, both hold flags and both lapse policies.
transitionTable :: [(Ctx, State, Event, Outcome)]
transitionTable =
  [ (ctx, s, ev, transition ctx s ev)
  | i <- [Temporary, Permanent]
  , m <- [Manual, Auto]
  , h <- [False, True]
  , l <- [LapseRevert, LapseHold]
  , let ctx = Ctx i m h l
  , s <- allStates
  , ev <- allEvents
  , transition ctx s ev /= NotApplicable
  ]

-- | The table as tab-separated text: intent, mode, earlier_hold, on_lapse,
-- state, event, outcome. Only applicable transitions are listed; a pair
-- absent from the table cannot occur.
renderTable :: Text
renderTable =
  T.unlines
    ( "intent\tmode\tearlier_hold\ton_lapse\tstate\tevent\toutcome"
        : [ T.intercalate
              "\t"
              [ intentT (ctxIntent c)
              , modeT (ctxMode c)
              , if ctxEarlierHold c then "yes" else "no"
              , lapseT (ctxOnLapse c)
              , T.pack (show s)
              , T.pack (drop 2 (show e))
              , outcomeT o
              ]
          | (c, s, e, o) <- transitionTable
          ]
    )
  where
    intentT i = case i of
      Temporary -> "temporary"
      Permanent -> "permanent"
    modeT m = case m of
      Manual -> "manual"
      Auto -> "auto"
    lapseT l = case l of
      LapseRevert -> "revert"
      LapseHold -> "hold"
    outcomeT o = case o of
      To s -> "-> " <> T.pack (show s)
      Stay -> "stay"
      Refuse c -> "refuse " <> T.pack (show c)
      NotApplicable -> "n/a"
