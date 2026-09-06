{-# LANGUAGE OverloadedStrings #-}
-- | Gates, docs/ROADMAP.md section 5.11: satisfiability, the minimum number
-- of distinct human authenticators on any satisfying path, the zero-human
-- path, the requester exclusion, and the earliest instant a gate is
-- satisfiable by wait alone.
--
-- A gate is a weighted threshold over factors; a factor is an authenticator,
-- any human, a nested group, or a wait. The prototype enumerates factor
-- subsets, which is exact and small enough for every tenant.
module Rue.Proto.Gates
  ( GateReport (..)
  , report
  , renderGate
  , unknownAuthenticators
  , countsRequester
  ) where

import Data.List (nub, subsequences, sort)
import Data.Maybe (mapMaybe)
import Data.Text (Text)
import qualified Data.Text as T
import Rue.Proto.Model

data GateReport = GateReport
  { satisfiable :: Bool
  , minDistinctHumans :: Maybe Int -- ^ over satisfying paths; 'Nothing' if unsatisfiable
  , zeroHumanPath :: Bool
  , waitAloneAt :: Maybe Duration -- ^ earliest instant satisfiable by waits only
  }
  deriving (Eq, Show)

-- | One way of satisfying a gate: which human authenticators it uses and the
-- longest wait it needs. Non-human authenticators contribute weight and no
-- human.
data Path = Path {pathHumans :: [Text], pathWait :: Maybe Duration}
  deriving (Eq, Show)

-- | Every satisfying path of a gate against the binding's authenticators.
satisfyingPaths :: [Authenticator] -> GateExpr -> [Path]
satisfyingPaths auths g = case g of
  Single f -> factorPaths f
  Thresh n factors ->
    concat
      [ combine chosen
      | chosen <- subsequences (zip [0 :: Int ..] factors)
      , sum (map (factorWeight . snd) chosen) >= n
      , minimalWeight n (map (factorWeight . snd) chosen)
      ]
  where
    -- A path per choice of one satisfying path for each chosen factor.
    combine chosen = foldr (\(_, f) acc -> [merge p q | p <- factorPaths f, q <- acc]) [Path [] Nothing] chosen
    merge (Path h1 w1) (Path h2 w2) = Path (nub (h1 <> h2)) (maxWait w1 w2)
    maxWait a b = case (a, b) of
      (Nothing, x) -> x
      (x, Nothing) -> x
      (Just x, Just y) -> Just (max x y)
    -- Skip supersets that add nothing: every factor is needed for the sum.
    minimalWeight n ws = all (\w -> sum ws - w < n) ws || null ws
    factorWeight f = case f of
      Auth _ w -> w
      Humans w -> w
      Group _ w -> w
      Wait _ w -> w
    factorPaths f = case f of
      Auth i _ -> case [a | a <- auths, authId a == i] of
        a : _ -> [Path [i | authHuman a] Nothing]
        [] -> [] -- unknown authenticator: satisfies nothing
      Humans _ -> [Path [authId a] Nothing | a <- auths, authHuman a]
      Group inner _ -> satisfyingPaths auths inner
      Wait d _ -> [Path [] (Just d)]

report :: [Authenticator] -> GateExpr -> GateReport
report auths g =
  GateReport
    { satisfiable = not (null ps)
    , minDistinctHumans = if null ps then Nothing else Just (minimum (map (length . pathHumans) ps))
    , zeroHumanPath = any (null . pathHumans) ps
    , waitAloneAt = case sort (mapMaybe waitOnly ps) of
        d : _ -> Just d
        [] -> Nothing
    }
  where
    ps = satisfyingPaths auths g
    waitOnly (Path hs w) = if null hs then w else Nothing

-- | Authenticator ids the gate names that the binding does not publish.
unknownAuthenticators :: [Authenticator] -> GateExpr -> [Text]
unknownAuthenticators auths g = nub [i | i <- named g, i `notElem` map authId auths]
  where
    named e = case e of
      Single f -> factorNamed f
      Thresh _ fs -> concatMap factorNamed fs
    factorNamed f = case f of
      Auth i _ -> [i]
      Group inner _ -> named inner
      _ -> []

-- | Whether the requester's identity is an authenticator the gate names.
-- @humans()@ counts the requester when the requester is a human
-- authenticator.
countsRequester :: [Authenticator] -> Text -> GateExpr -> Bool
countsRequester auths requester g = go g
  where
    requesterHuman = any (\a -> authId a == requester && authHuman a) auths
    go e = case e of
      Single f -> factor f
      Thresh _ fs -> any factor fs
    factor f = case f of
      Auth i _ -> i == requester
      Humans _ -> requesterHuman
      Group inner _ -> go inner
      Wait _ _ -> False

-- | The gate as the surface spells it.
renderGate :: GateExpr -> Text
renderGate g = case g of
  Single f -> factor f
  Thresh n fs -> "thresh(" <> T.pack (show n) <> ", " <> T.intercalate ", " (map factor fs) <> ")"
  where
    factor f = case f of
      Auth i w -> "auth(:" <> i <> weight w <> ")"
      Humans w -> "humans(" <> T.drop 2 (weight w) <> ")"
      Group inner w -> "group(" <> renderGate inner <> weight w <> ")"
      Wait d w -> "wait(" <> renderDuration d <> weight w <> ")"
    weight w = if w == 1 then "" else ", weight: " <> T.pack (show w)
