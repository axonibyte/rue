module Main (main) where

import Test.Tasty (defaultMain, testGroup)

import qualified Test.Canonical
import qualified Test.Check
import qualified Test.Diagnostics
import qualified Test.Golden
import qualified Test.Laws
import qualified Test.Ledger
import qualified Test.Schema
import qualified Test.States
import qualified Test.Tenants

main :: IO ()
main = do
  golden <- Test.Golden.tests
  schema <- Test.Schema.tests
  tenantsT <- Test.Tenants.tests
  defaultMain $
    testGroup
      "rue-proto"
      [ testGroup "tier1" [Test.Canonical.tests, Test.Diagnostics.tests, Test.Laws.tests, Test.Check.tests]
      , testGroup "tier2" [golden, tenantsT]
      , testGroup "tier3" [schema]
      , testGroup "tier4" [Test.States.tests, Test.Ledger.tests]
      ]
