{-# LANGUAGE OverloadedStrings #-}
-- | The cross-plan ledger: reservation at request, refusal of overlap and of
-- a held exclusivity class, rehearsals reserving nothing, release on close.
module Test.Ledger (tests) where

import Data.Text (Text)
import Rue.Proto.Interference (Fact (..))
import Rue.Proto.Ledger
import Test.Tasty (TestTree, testGroup)
import Test.Tasty.HUnit (assertBool, assertFailure, testCase, (@?=))

inst :: Text -> [Fact] -> Instance
inst i fs = Instance {instId = i, instHost = "db-01", instUmbra = fs, instExclusivity = Nothing, instRehearsal = False}

-- | Reserve one instance in an empty ledger, or fail the test.
reserved :: Instance -> IO Ledger
reserved i = either (\(c, why) -> assertFailure ("could not reserve " <> show c <> ": " <> show why)) pure (request i empty)

pf :: Fact
pf = Fact "file:/etc/pf.conf" Nothing

keysA, keysB :: Fact
keysA = Fact "file:/root/.ssh/authorized_keys" (Just "rue-a")
keysB = Fact "file:/root/.ssh/authorized_keys" (Just "rue-b")

tests :: TestTree
tests =
  testGroup
    "ledger"
    [ testCase "a second plan overlapping a pending umbra is refused at request with R0203" $
        case request (inst "a" [pf]) empty >>= request (inst "b" [pf]) of
          Left (code, _) -> code @?= R0203
          Right _ -> assertBool "admitted an overlap" False
    , testCase "distinct regions on one fact coexist" $
        assertBool "refused disjoint regions" (either (const False) (const True) (request (inst "a" [keysA]) empty >>= request (inst "b" [keysB])))
    , testCase "a held exclusivity class refuses with R0101 regardless of umbra" $
        case request (inst "a" []) {instExclusivity = Just "corpse-1"} empty >>= request (inst "b" [pf]) {instExclusivity = Just "corpse-1"} of
          Left (code, _) -> code @?= R0101
          Right _ -> assertBool "admitted a class collision" False
    , testCase "a rehearsal reserves nothing and is never blocked" $ do
        l <- reserved (inst "a" [pf])
        case request (inst "r" [pf]) {instRehearsal = True} l of
          Right l' -> length (holdings l') @?= 1
          Left _ -> assertBool "rehearsal blocked" False
    , testCase "release admits what was refused" $ do
        l <- reserved (inst "a" [pf])
        assertBool "still refused after release" (either (const False) (const True) (request (inst "b" [pf]) (release "a" l)))
    , testCase "different hosts never interfere" $
        assertBool "cross-host refusal" (either (const False) (const True) (request (inst "a" [pf]) empty >>= request (inst "b" [pf]) {instHost = "db-02"}))
    ]
