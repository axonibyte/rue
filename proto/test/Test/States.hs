-- | Tier 4: the five class rules of section 5.9, asserted over every context,
-- state and event the machine admits. The table itself is a golden.
module Test.States (tests) where

import Rue.Proto.Intent (Intent (..))
import Rue.Proto.Model (Mode (..), OnLapse (..))
import Rue.Proto.States
import Test.Tasty (TestTree, testGroup)
import Test.Tasty.HUnit (assertBool, testCase, (@?=))

manualTemporary :: Ctx
manualTemporary = Ctx Temporary Manual False LapseRevert

ctxs :: [Ctx]
ctxs = [Ctx i m h l | i <- [Temporary, Permanent], m <- [Manual, Auto], h <- [False, True], l <- [LapseRevert, LapseHold]]

tests :: TestTree
tests =
  testGroup
    "states"
    [ testCase "rule 1: terminal states admit no event" $
        assertBool "a terminal state moved" $
          and [transition c s e == NotApplicable | c <- ctxs, s <- [Closed, Committed], e <- allEvents]
    , testCase "rule 2: in a temporary plan, wane expires every non-terminal state but DriftHeld, Stuck and the pre-approval states" $
        assertBool "a bounded state survived wane" $
          and
            [ transition c s EvWaneElapses == To Expired
            | c <- ctxs
            , ctxIntent c == Temporary
            , s <- allStates
            , not (terminal s)
            , s `notElem` [DriftHeld, Stuck, Pending, Unchecked, Checked, ApprovalExpired, Expired]
            ]
    , testCase "rule 2: wane never holds" $
        assertBool "wane produced a hold" $
          and [transition c s EvWaneElapses /= To Held | c <- ctxs, s <- allStates]
    , testCase "rule 2: a permanent plan has no wane event" $
        assertBool "wane occurred on a permanent plan" $
          and [transition c s EvWaneElapses == NotApplicable | c <- ctxs, ctxIntent c == Permanent, s <- allStates]
    , testCase "rule 3: DriftHeld and Stuck outlive wane in any plan" $
        assertBool "DriftHeld or Stuck expired" $
          and [transition c s EvWaneElapses `elem` [Stay, NotApplicable] | c <- ctxs, s <- [DriftHeld, Stuck]]
    , testCase "rule 3: Held and Deferred in a permanent plan wait for an operator" $
        assertBool "a permanent hold ended by time" $
          and
            [ transition c s e == NotApplicable
            | c <- ctxs
            , ctxIntent c == Permanent
            , s <- [Held, Deferred]
            , e <- [EvWaneElapses, EvBoundLapses]
            ]
    , testCase "rule 3: only a human ends DriftHeld or Stuck" $ do
        assertBool "DriftHeld exit" $ and [transition c DriftHeld e `elem` [To Reverting, To Closed, Stay, NotApplicable, Refuse R0103] | c <- ctxs, e <- allEvents]
        assertBool "Stuck exit" $ and [transition c Stuck e `elem` [To Reverting, To Closed, Stay, NotApplicable] | c <- ctxs, e <- allEvents]
    , testCase "rule 4: refusal during Applying reverts unless an earlier step holds" $
        assertBool "refusal outcome" $
          and
            [ transition c Applying EvRefuse == (if ctxEarlierHold c then To Held else To Reverting)
            | c <- ctxs
            ]
    , testCase "rule 4: a lapse before wane follows on_lapse, and always reverts under auto" $
        assertBool "lapse outcome" $
          and
            [ transition c Waiting EvBoundLapses
                == (if ctxMode c == Auto || ctxOnLapse c == LapseRevert then To Reverting else To Held)
            | c <- ctxs
            ]
    , testCase "rule 5: commit is reached from Applying by the item and from Held or Deferred by the verb, permanent only" $ do
        assertBool "permanent commit" $
          and
            [ transition c Applying EvCommitItem == To Committed
                && transition c Held EvCommitVerb == To Committed
                && transition c Deferred EvCommitVerb == To Committed
            | c <- ctxs
            , ctxIntent c == Permanent
            ]
        assertBool "temporary commit refused" $
          and
            [ transition c s e == Refuse R0102
            | c <- ctxs
            , ctxIntent c == Temporary
            , (s, e) <- [(Applying, EvCommitItem), (Held, EvCommitVerb), (Deferred, EvCommitVerb)]
            ]
    , testCase "rule 5: renew is temporary-only and confirm is permanent-only" $ do
        assertBool "renew" $ and [transition c Applying EvRenew == (if ctxIntent c == Temporary then Stay else Refuse R0102) | c <- ctxs]
        assertBool "renew while applied" $ and [transition c Applied EvRenew == Stay | c <- ctxs, ctxIntent c == Temporary]
        assertBool "confirm" $ and [transition c Applying EvConfirm == (if ctxIntent c == Permanent then Stay else Refuse R0102) | c <- ctxs]
    , testCase "a permanent plan has no Applied or Suspended state" $
        assertBool "event admitted in an unreachable state" $
          and [transition c s e == NotApplicable | c <- ctxs, ctxIntent c == Permanent, s <- [Applied, Suspended], e <- allEvents]
    , testCase "recant on DriftHeld without force is R0103; with force it reverts" $ do
        transition manualTemporary DriftHeld EvRecant @?= Refuse R0103
        transition manualTemporary DriftHeld EvForceDrift @?= To Reverting
    , testCase "abandon closes Stuck and DriftHeld and nothing else" $
        assertBool "abandon" $
          and [transition c s EvAbandon == (if s `elem` [Stuck, DriftHeld] then To Closed else NotApplicable) | c <- ctxs, s <- allStates]
    , testCase "a temporary plan never reaches Committed; a permanent plan never reaches Applied" $ do
        assertBool "temporary committed" $ and [o /= To Committed | (c, _, _, o) <- transitionTable, ctxIntent c == Temporary]
        assertBool "permanent applied" $ and [o /= To Applied | (c, _, _, o) <- transitionTable, ctxIntent c == Permanent]
    , testCase "the table lists only applicable transitions" $
        assertBool "n/a in table" $ and [o /= NotApplicable | (_, _, _, o) <- transitionTable]
    ]
