{-# LANGUAGE OverloadedStrings #-}
-- | The cross-plan ledger, docs/ROADMAP.md section 5.12, as a pure value:
-- per host, the umbras of active and pending instances and the exclusivity
-- classes they hold. A new instance overlapping a reserved umbra is refused
-- at request (R0203), regardless of exclusivity class, before any proof is
-- collected; a second instance in a held class is refused with R0101 (exit
-- 75). A request dry-run reserves nothing and is never blocked by nothing.
module Rue.Proto.Ledger
  ( Ledger
  , Instance (..)
  , LedgerCode (..)
  , empty
  , request
  , release
  , holdings
  ) where

import Data.Text (Text)
import Rue.Proto.Interference (Fact (..))
import Rue.Proto.Model (Host)

data Instance = Instance
  { instId :: Text
  , instHost :: Host
  , instUmbra :: [Fact]
  , instExclusivity :: Maybe Text
  , instRehearsal :: Bool
  }
  deriving (Eq, Show)

data LedgerCode
  = R0101 -- ^ exclusivity held by another instance (exit 75)
  | R0203 -- ^ cross-plan umbra overlap with an active or pending instance
  deriving (Eq, Show)

newtype Ledger = Ledger [Instance]
  deriving (Eq, Show)

empty :: Ledger
empty = Ledger []

-- | Reserve at request. A rehearsal is admitted and records nothing.
request :: Instance -> Ledger -> Either (LedgerCode, Text) Ledger
request inst l@(Ledger held)
  | instRehearsal inst = Right l
  | otherwise = case [h | h <- sameHost, sameClass h] of
      h : _ -> Left (R0101, "exclusivity class held by " <> instId h)
      [] -> case [h | h <- sameHost, any (\f -> any (touches f) (instUmbra h)) (instUmbra inst)] of
        h : _ -> Left (R0203, "umbra overlaps " <> instId h)
        [] -> Right (Ledger (inst : held))
  where
    sameHost = [h | h <- held, instHost h == instHost inst]
    sameClass h = case (instExclusivity h, instExclusivity inst) of
      (Just a, Just b) -> a == b
      _ -> False

-- | Facts touch on equal shapes; two regions must also share an anchor.
touches :: Fact -> Fact -> Bool
touches (Fact s1 a1) (Fact s2 a2)
  | s1 /= s2 = False
  | otherwise = case (a1, a2) of
      (Just x, Just y) -> x == y
      _ -> True

-- | Release on cancel, lapse, close or commit.
release :: Text -> Ledger -> Ledger
release i (Ledger held) = Ledger [h | h <- held, instId h /= i]

holdings :: Ledger -> [Instance]
holdings (Ledger held) = held
