module Main (main) where

import Test.Tasty (defaultMain, testGroup)

import qualified Test.Canonical
import qualified Test.Check
import qualified Test.Diagnostics
import qualified Test.Laws
import qualified Test.Ledger
import qualified Test.States

-- The prototype's own tiers. The golden comparison (tiers 2 and 3) moved to
-- the Rust crates when the prototype became the Phase 0 record; see
-- docs/TESTING.md.
main :: IO ()
main =
  defaultMain $
    testGroup
      "rue-proto"
      [ testGroup "tier1" [Test.Canonical.tests, Test.Diagnostics.tests, Test.Laws.tests, Test.Check.tests]
      , testGroup "tier4" [Test.States.tests, Test.Ledger.tests]
      ]
