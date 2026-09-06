-- | Backstops and ordering, docs/ROADMAP.md section 5.6: which steps a
-- backstop covers, where it is installed and armed, the late-arming window,
-- the reach rule, and the triggers an intent admits.
module Rue.Proto.Backstop
  ( Coverage (..)
  , coverage
  , reachViolations
  , triggerViolations
  , heartbeatViolations
  , TriggerViolation (..)
  ) where

import Rue.Proto.Algebra (numbered)
import Rue.Proto.Intent (Intent (..), effectiveWane)
import Rue.Proto.Model

-- | What a plan's backstop covers, if it has one.
data Coverage = Coverage
  { covered :: [Int] -- ^ steps with a @:target@ undo, in order
  , installedBefore :: Maybe Int -- ^ the first covered step
  , armedBeforeStep :: Int -- ^ the plan's @arm_before@
  , lateArmingWindow :: [Int] -- ^ covered steps that complete before arming
  }
  deriving (Eq, Show)

targetUndoSteps :: [Item] -> [Int]
targetUndoSteps items =
  [ n
  | (n, it) <- numbered items
  , Just o <- [opOf it]
  , opUndoLocus o == UndoTarget
  ]

opOf :: Item -> Maybe Op
opOf it = case it of
  Step s -> Just (stepOp s)
  KnellItem s -> Just (stepOp s)
  _ -> Nothing

coverage :: Plan -> Maybe Coverage
coverage p = case planBackstop p of
  Nothing -> Nothing
  Just b ->
    let cov = targetUndoSteps (planBody p)
        ArmBefore a = armBefore b
     in Just
          Coverage
            { covered = cov
            , installedBefore = case cov of
                n : _ -> Just n
                [] -> Nothing
            , armedBeforeStep = a
            , lateArmingWindow = [n | n <- cov, n < a]
            }

-- | Steps with @reach@ that violate the reach rule (E0401): no @:target@
-- undo, no backstop, or a backstop armed after the step.
reachViolations :: Plan -> [Int]
reachViolations p =
  [ n
  | (n, it) <- numbered (planBody p)
  , Just o <- [opOf it]
  , not (null (opReach o))
  , opUndoLocus o /= UndoTarget || not (armedBefore n)
  ]
  where
    armedBefore n = case planBackstop p of
      Nothing -> False
      Just b -> let ArmBefore a = armBefore b in a <= n

data TriggerViolation
  = AfterNotWane -- ^ E0503: temporary plan's @after:@ differs from its wane
  | TemporaryConfirmed -- ^ E0503: @unless_confirmed@ on a temporary plan that does not fire by construction
  | PermanentAfter -- ^ E0504-adjacent: a permanent plan may not expire on a timer
  | NoConfirmOrCommitPath Int -- ^ E0504: paths reaching neither confirm nor commit
  deriving (Eq, Show)

-- | Trigger and path violations by intent.
triggerViolations :: Intent -> Plan -> [TriggerViolation]
triggerViolations intent p = case planBackstop p of
  Nothing -> []
  Just b -> case intent of
    Temporary
      | planFiresByConstruction p -> [AfterNotWane | After _ <- triggers b]
      | otherwise ->
          [AfterNotWane | After d <- triggers b, Just d /= effectiveWane p]
            <> [TemporaryConfirmed | UnlessConfirmed _ <- triggers b]
            <> [AfterNotWane | null [() | After _ <- triggers b]]
    Permanent ->
      [PermanentAfter | After _ <- triggers b]
        <> [NoConfirmOrCommitPath k | let k = pathsWithoutDisarm (planBody p), k > 0, not (planFiresByConstruction p)]

-- | Paths that reach neither @confirm()@ nor @commit()@.
pathsWithoutDisarm :: [Item] -> Int
pathsWithoutDisarm items = length (filter (\path -> Confirm `notElem` path && Commit `notElem` path) (paths items))
  where
    paths its = case its of
      [] -> [[]]
      it : rest -> [here <> there | here <- itemPaths it, there <- paths rest]
    itemPaths it = case it of
      Par xs -> paths xs
      Repeat _ _ body -> paths body
      When _ _ _ t e -> paths t <> paths e
      _ -> [[it]]

-- | Heartbeat intervals above a third of their deadline (E0405).
heartbeatViolations :: Plan -> [Trigger]
heartbeatViolations p = case planBackstop p of
  Nothing -> []
  Just b -> [t | t@(UnlessHeartbeat (Duration d) (Just (Duration i))) <- triggers b, i * 3 > d]
