-- | The reversal algebra, docs/ROADMAP.md section 5.5.
--
-- Laws the prototype must exhibit (Test.Laws):
--
-- > reverse (seq a b) = seq (reverse b) (reverse a)
-- > reverse (par xs)  = par (map reverse xs)
-- > reverse knell     = refuse
-- > reverse . reverse = id            -- on knell-free plans
--
-- Reversal is syntactic: a step's direction flips, a sequence's order flips,
-- a @par@ reverses each child in place, and a knell refuses. Partial
-- reversal, @reverse_from k@, undoes the applied prefix last-in-first-out; it
-- is the operation the runtime actually performs.
module Rue.Proto.Algebra
  ( Refused (..)
  , reverseItem
  , reverseItems
  , reverseFrom
  , seq_
  , par_
  , knellFree
  , leaves
  , numbered
  ) where

import Rue.Proto.Model

-- | Why a reversal is refused: the knell at this position is irreversible.
newtype Refused = Refused {refusedAt :: Item}
  deriving (Eq, Show)

-- | @seq@ is list append; @a |> b@ desugars to it and nothing else.
seq_ :: [Item] -> [Item] -> [Item]
seq_ = (<>)

par_ :: [Item] -> Item
par_ = Par

flipStep :: StepI -> StepI
flipStep s = s {stepDirection = other (stepDirection s)}
  where
    other Forward = Inverse
    other Inverse = Forward

-- | Reverse one item. Non-mutating items (confirm, commit, observe, assert,
-- preflight, a slot) reverse to themselves: they have no undo because they
-- changed nothing.
reverseItem :: Item -> Either Refused Item
reverseItem it = case it of
  Step s -> Right (Step (flipStep s))
  Par xs -> Par <$> traverse reverseItem xs
  KnellItem _ -> Left (Refused it)
  Repeat f v body -> Repeat f v <$> reverseItems body
  When g w l t e -> When g w l <$> reverseItems t <*> reverseItems e
  Slot _ -> Right it
  Confirm -> Right it
  Commit -> Right it
  Preflight _ -> Right it
  Observe _ _ -> Right it
  Assert {} -> Right it

-- | Reverse a sequence: last-in-first-out, each item reversed.
reverseItems :: [Item] -> Either Refused [Item]
reverseItems items = traverse reverseItem (reverse items)

-- | Undo the applied prefix of a plan: the first @k@ leaves, in reverse.
reverseFrom :: Int -> [Item] -> Either Refused [Item]
reverseFrom k items = reverseItems (take k (leaves items))

-- | A plan with no knell anywhere in it.
knellFree :: [Item] -> Bool
knellFree = all ok
  where
    ok it = case it of
      KnellItem _ -> False
      Par xs -> all ok xs
      Repeat _ _ body -> all ok body
      When _ _ _ t e -> all ok t && all ok e
      _ -> True

-- | The leaves of a plan in execution order: containers (par, repeat, when)
-- contribute their children; everything else is a leaf.
leaves :: [Item] -> [Item]
leaves = concatMap go
  where
    go it = case it of
      Par xs -> concatMap go xs
      Repeat _ _ body -> concatMap go body
      When _ _ _ t e -> concatMap go t <> concatMap go e
      _ -> [it]

-- | The leaves numbered from 1, which is how the verdict and @explain@ refer
-- to steps.
numbered :: [Item] -> [(Int, Item)]
numbered = zip [1 ..] . leaves
