module Main (main) where

import Test.Tasty (defaultMain, testGroup)

import qualified Test.Canonical
import qualified Test.Check
import qualified Test.Diagnostics
import qualified Test.Golden
import qualified Test.Laws
import qualified Test.Ledger
import qualified Test.States

main :: IO ()
main = do
  golden <- Test.Golden.tests
  defaultMain $
    testGroup
      "rue-proto"
      [ testGroup "tier1" [Test.Canonical.tests, Test.Diagnostics.tests, Test.Laws.tests, Test.Check.tests]
      , testGroup "tier2" [golden]
      , testGroup "tier4" [Test.States.tests, Test.Ledger.tests]
      ]
