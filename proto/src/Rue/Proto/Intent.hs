-- | Intent, docs/ROADMAP.md section 5.4: a plan with @wane@ is temporary; a
-- plan with a reachable @commit()@ is permanent; both or neither is E0501,
-- except a plan declaring @fires_by_construction@, which is temporary with
-- its @unless_confirmed@ duration as @wane@. A permanent plan reaches
-- @commit()@ on every non-refusing path (E0505) and @commit()@ is the last
-- item on its path (E0502).
module Rue.Proto.Intent
  ( Intent (..)
  , inferIntent
  , effectiveWane
  , commitReachable
  , commitNotLast
  , pathsWithoutCommit
  , commitStep
  ) where

import Rue.Proto.Algebra (numbered)
import Rue.Proto.Model

data Intent = Temporary | Permanent
  deriving (Eq, Ord, Show)

-- | The intent, or 'Nothing' when it cannot be determined (E0501).
inferIntent :: Plan -> Maybe Intent
inferIntent p
  | planFiresByConstruction p = if hasWane || hasCommit then Nothing else Just Temporary
  | hasWane && not hasCommit = Just Temporary
  | hasCommit && not hasWane = Just Permanent
  | otherwise = Nothing
  where
    hasWane = planWane p /= Nothing
    hasCommit = commitReachable (planBody p)

-- | The bound a temporary plan's states expire at: @wane@, or for a
-- fires-by-construction plan the @unless_confirmed@ duration.
effectiveWane :: Plan -> Maybe Duration
effectiveWane p = case planWane p of
  Just w -> Just w
  Nothing
    | planFiresByConstruction p -> case planBackstop p of
        Just b -> firstConfirmed (triggers b)
        Nothing -> Nothing
    | otherwise -> Nothing
  where
    firstConfirmed ts = case [d | UnlessConfirmed d <- ts] of
      d : _ -> Just d
      [] -> Nothing

commitReachable :: [Item] -> Bool
commitReachable = any reaches
  where
    reaches it = case it of
      Commit -> True
      Par xs -> any reaches xs
      Repeat _ _ body -> any reaches body
      When _ _ _ t e -> any reaches t || any reaches e
      _ -> False

-- | The step number of the first @commit()@, if any.
commitStep :: [Item] -> Maybe Int
commitStep items = case [n | (n, Commit) <- numbered items] of
  n : _ -> Just n
  [] -> Nothing

-- | Every path through a plan: the sequence of leaves along each choice of
-- @when@ arm. Repeat bodies contribute once; par children contribute in
-- order.
paths :: [Item] -> [[Item]]
paths items = case items of
  [] -> [[]]
  it : rest -> [here <> there | here <- itemPaths it, there <- paths rest]
  where
    itemPaths it = case it of
      Par xs -> paths xs
      Repeat _ _ body -> paths body
      When _ _ _ t e -> paths t <> paths e
      _ -> [[it]]

-- | Paths on which @commit()@ is followed by another item (E0502).
commitNotLast :: [Item] -> Bool
commitNotLast items = any bad (paths items)
  where
    bad p = case dropWhile (/= Commit) p of
      Commit : _ : _ -> True
      _ -> False

-- | Paths that never reach @commit()@ (E0505 for a permanent plan). A path
-- ending in a knell-free refusal is still a path; the roadmap's
-- "non-refusing path" is every path the checker can enumerate, since refusal
-- is a runtime outcome, not a syntactic one.
pathsWithoutCommit :: [Item] -> Int
pathsWithoutCommit items = length (filter (notElem Commit) (paths items))
