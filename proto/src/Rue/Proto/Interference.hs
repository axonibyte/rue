{-# LANGUAGE OverloadedStrings #-}
-- | The interference query, docs/ROADMAP.md section 5.7, as list
-- comprehensions over the numbered leaves of a plan.
--
-- > writes(S, F)       :- step(S), umbra(S, F).
-- > maywrite(S, F)     :- step(S), penumbra(S, F).
-- > needs(S, F)        :- step(S), undo_pre(S, F).
-- > before(A, B)       :- lifo_order(A, B).          -- undefined between par siblings
-- > conflict(A, B, F)  :- writes(A, F), writes(B, F), before(A, B), needs(A, F).
-- > mayconflict(A,B,F) :- maywrite(A,F), writes(B,F), before(A,B), needs(A,F).
-- > mayconflict(A,B,F) :- writes(A,F), maywrite(B,F), before(A,B), needs(A,F).
-- > par_ok(P)          :- par(P), forall X,Y in children(P), X != Y => disjoint_umbra(X, Y).
--
-- A fact is a footprint shape on a host: the host a step's locus resolves
-- to (the plan's owner for @:target@, the literal @controller@ for
-- @:controller@, the named host for @host(...)@). The same shape on two hosts
-- is two facts (section 5.12). A shape whose text contains a runtime-bound
-- part, written @{...}@, is penumbral: its instance is bound only when a
-- value flows in; so is any fact of a step whose host is bound at runtime. Region entries on one shape with distinct anchors are
-- disjoint (section 5.2); the same anchor twice in one plan is E0305.
-- Iterations of one @repeat over:@ loop bind distinct instances of a shape
-- indexed by the loop variable and are disjoint by construction.
module Rue.Proto.Interference
  ( Fact (..)
  , Leaf (..)
  , Conflict (..)
  , leafHostText
  , umbra
  , penumbra
  , needs
  , conflicts
  , mayConflicts
  , parViolations
  , anchorDuplicates
  , disjointUmbra
  , stepFacts
  , parSiblings
  ) where

import Data.List (nub, tails)
import Data.Maybe (mapMaybe)
import Data.Text (Text)
import qualified Data.Text as T
import Rue.Proto.Model

-- | A fact as the query sees it: a shape, and for a region its anchor.
data Fact = Fact {factShape :: Text, factAnchor :: Maybe Text}
  deriving (Eq, Ord, Show)

data Conflict = Conflict {conflictEarlier :: Int, conflictLater :: Int, conflictFact :: Fact}
  deriving (Eq, Ord, Show)

-- | The step's op, if the leaf is a step or a knell.
stepOf :: Item -> Maybe StepI
stepOf it = case it of
  Step s -> Just s
  KnellItem s -> Just s
  _ -> Nothing

-- | A shape whose instance is bound only at runtime.
runtimeBound :: Text -> Bool
runtimeBound = T.isInfixOf "{"

-- | The facts an op definitely writes.
umbra :: Op -> [Fact]
umbra o =
  [ Fact (fpShape e) (fpAnchor e)
  | e <- opFootprint o
  , writes (fpKind e)
  , not (runtimeBound (fpShape e))
  ]

-- | The facts an op may write: written kinds whose shape is runtime-bound.
penumbra :: Op -> [Fact]
penumbra o =
  [ Fact (fpShape e) (fpAnchor e)
  | e <- opFootprint o
  , writes (fpKind e)
  , runtimeBound (fpShape e)
  ]

writes :: Kind -> Bool
writes k = case k of
  Owned -> True
  Region -> True
  Modified -> True
  AppendOnly -> True
  Held -> True
  Derived -> False

-- | What an op's undo needs unchanged. @Restore@ derives it from the
-- footprint: every fact it wrote must still equal its post-do value.
-- Computed and compensating undos declare it.
needs :: Op -> [Fact]
needs o = case opUndo o of
  Restore -> umbra o <> penumbra o
  Computed pre -> map (`Fact` Nothing) pre
  Compensate pre -> map (`Fact` Nothing) pre
  NoUndo -> []

-- | Two facts touch when their shapes overlap and, for two regions, their
-- anchors are equal; a region and a non-region on one shape touch. Two static
-- shapes overlap when equal; a runtime-bound shape overlaps anything that
-- shares its static prefix, which is what makes it penumbral.
touches :: Fact -> Fact -> Bool
touches (Fact s1 a1) (Fact s2 a2)
  | not (overlapShape s1 s2) = False
  | otherwise = case (a1, a2) of
      (Just x, Just y) -> x == y
      _ -> True

overlapShape :: Text -> Text -> Bool
overlapShape s1 s2
  | not (runtimeBound s1) && not (runtimeBound s2) = s1 == s2
  | otherwise = static s1 `T.isPrefixOf` s2 || static s2 `T.isPrefixOf` s1
  where
    static = T.takeWhile (/= '{')

-- | A numbered step as the query sees it: its op, the host its facts live
-- on, and the loop variables whose iterations make a shape distinct by
-- construction.
data Leaf = Leaf {leafN :: Int, leafOp :: Op, leafHost :: Text, leafVars :: [Text]}
  deriving (Show)

-- | The host a step's locus resolves to, as text; a bound host is written
-- @{name}@ so that it is runtime-bound like a penumbral shape.
leafHostText :: Host -> Op -> Text
leafHostText owner o = case opLocus o of
  Controller -> "controller"
  Target -> owner
  HostLocus (StaticHost h) -> h
  HostLocus (BoundHost b) -> "{" <> b <> "}"

-- | Two hosts overlap when equal, or when either is bound at runtime.
overlapHost :: Text -> Text -> Bool
overlapHost h1 h2 = h1 == h2 || runtimeBound h1 || runtimeBound h2

-- | Whether two leaves can touch the same fact at all.
sameHost :: Leaf -> Leaf -> Bool
sameHost x y = overlapHost (leafHost x) (leafHost y)

-- | Whether a leaf's facts are all penumbral by virtue of its host.
boundHost :: Leaf -> Bool
boundHost = runtimeBound . leafHost

-- | The numbered steps of a plan owned by the given host.
stepFacts :: Host -> [Item] -> [Leaf]
stepFacts owner items = go [] items 1
  where
    go vars its n = case its of
      [] -> []
      it : rest -> case it of
        Par xs -> let inner = go vars xs n in inner <> go vars rest (n + length (flat xs))
        Repeat (Over _ _ _) v body -> let inner = go (v : vars) body n in inner <> go vars rest (n + length (flat body))
        Repeat (Count _) _ body -> let inner = go vars body n in inner <> go vars rest (n + length (flat body))
        When _ _ _ t e ->
          let ti = go vars t n
              ei = go vars e (n + length (flat t))
           in ti <> ei <> go vars rest (n + length (flat t) + length (flat e))
        _ -> case stepOf it of
          Just s -> Leaf n (stepOp s) (leafHostText owner (stepOp s)) vars : go vars rest (n + 1)
          Nothing -> go vars rest (n + 1)
    flat = concatMap leafCount
    leafCount it = case it of
      Par xs -> flat xs
      Repeat _ _ body -> flat body
      When _ _ _ t e -> flat t <> flat e
      _ -> [()]

-- | A fact indexed by a loop variable of its own iteration is distinct
-- across iterations; two such facts from the same loop body are disjoint.
indexedBy :: [Text] -> Fact -> Bool
indexedBy vars f = any (\v -> ("{" <> v <> "}") `T.isInfixOf` factShape f) vars

-- | Definite conflicts (E0301): an earlier step's undo needs a fact a later
-- step also definitely writes, on one statically known host. Two children of
-- one @par@ have no order between them and are judged by 'parViolations'.
conflicts :: Host -> [Item] -> [Conflict]
conflicts owner items =
  nub
    [ Conflict (leafN x) (leafN y) f
    | (x : later) <- tails (stepFacts owner items)
    , y <- later
    , (leafN x, leafN y) `notElem` siblings
    , not (boundHost x)
    , not (boundHost y)
    , leafHost x == leafHost y
    , let oa = leafOp x
    , let ob = leafOp y
    , f <- umbra oa
    , f `elem` needs oa
    , g <- umbra ob
    , touches f g
    , not (sameLoop (leafVars x) (leafVars y) && indexedBy (leafVars x) f)
    ]
  where
    siblings = parSiblings owner items

sameLoop :: [Text] -> [Text] -> Bool
sameLoop va vb = not (null va) && va == vb

-- | May-conflicts (E0302 under strict, a verdict clause under warn): the
-- same query where at least one side is penumbral, by shape or by host.
mayConflicts :: Host -> [Item] -> [Conflict]
mayConflicts owner items =
  nub
    [ Conflict (leafN x) (leafN y) f
    | (x : later) <- tails (stepFacts owner items)
    , y <- later
    , (leafN x, leafN y) `notElem` siblings
    , sameHost x y
    , let oa = leafOp x
    , let ob = leafOp y
    , f <- needs oa
    , g <- penumbra oa <> umbra oa
    , touches f g
    , h <- penumbra ob <> umbra ob
    , touches f h
    , (h `elem` penumbra ob) || (g `elem` penumbra oa) || boundHost x || boundHost y
    , not (sameLoop (leafVars x) (leafVars y) && indexedBy (leafVars x) f)
    ]
  where
    siblings = parSiblings owner items

-- | Pairwise disjoint umbras: trivially so on distinct static hosts.
disjointUmbra :: Host -> Op -> Op -> Bool
disjointUmbra owner x y =
  not (overlapHost (leafHostText owner x) (leafHostText owner y))
    || and [not (touches f g) | f <- umbra x, g <- umbra y]

-- | Par violations: pairs of children whose umbras overlap (E0303), and any
-- child with @reach@ (E0304). Each child's ops are collected recursively.
parViolations :: Host -> [Item] -> ([(Int, Int)], [Int])
parViolations owner items = (overlaps, reachy)
  where
    parGroups = parChildren owner items
    overlaps =
      nub
        [ (a, b)
        | grp <- parGroups
        , (ca : rest) <- tails grp
        , cb <- rest
        , (a, oa) <- ca
        , (b, ob) <- cb
        , not (disjointUmbra owner oa ob)
        ]
    reachy = nub [n | grp <- parGroups, child <- grp, (n, o) <- child, not (null (opReach o))]

-- | Pairs of steps (earlier, later) that sit in different children of one
-- @par@ and so have no order between them.
parSiblings :: Host -> [Item] -> [(Int, Int)]
parSiblings owner items =
  nub
    [ (min a b, max a b)
    | grp <- parChildren owner items
    , (ca : rest) <- tails grp
    , cb <- rest
    , (a, _) <- ca
    , (b, _) <- cb
    ]

-- | Every @par@ in the plan, as its children, each child its (step, op)s.
parChildren :: Host -> [Item] -> [[[(Int, Op)]]]
parChildren owner items = collect items 1
  where
    numberedLeaves = stepFacts owner items
    collect its n = case its of
      [] -> []
      it : rest -> case it of
        Par xs ->
          let children = childOps xs n
           in children : collect rest (n + length (concatMap leafOps xs)) <> concat (zipWith (\x k -> collect [x] k) xs (childStarts xs n))
        Repeat _ _ body -> collect body n <> collect rest (n + length (concatMap leafOps body))
        When _ _ _ t e -> collect t n <> collect e (n + length (concatMap leafOps t)) <> collect rest (n + length (concatMap leafOps t) + length (concatMap leafOps e))
        _ -> collect rest (n + 1)
    childStarts xs n = scanl (\k x -> k + length (leafOps x)) n xs
    childOps xs n = zipWith (\x k -> opsFrom k x) xs (childStarts xs n)
    opsFrom k x = mapMaybe (\l -> if leafN l >= k && leafN l < k + length (leafOps x) then Just (leafN l, leafOp l) else Nothing) numberedLeaves
    leafOps it = case it of
      Par xs -> concatMap leafOps xs
      Repeat _ _ body -> concatMap leafOps body
      When _ _ _ t e -> concatMap leafOps t <> concatMap leafOps e
      _ -> [()]

-- | The same anchor declared twice on one fact within a plan (E0305): the
-- offending (step, step, fact) triples.
anchorDuplicates :: Host -> [Item] -> [Conflict]
anchorDuplicates owner items =
  nub
    [ Conflict (leafN x) (leafN y) f
    | (x : later) <- tails (stepFacts owner items)
    , y <- later
    , sameHost x y
    , f@(Fact _ (Just _)) <- regions (leafOp x)
    , g <- regions (leafOp y)
    , f == g
    ]
  where
    regions o = [Fact (fpShape e) (fpAnchor e) | e <- opFootprint o, fpKind e == Region]
