-- | The diagnostics enumeration is well-formed: every code renders as E plus
-- four digits, the texts are distinct, and the enumeration is in table
-- order. Agreement with docs/ROADMAP.md is tools/lint-ecodes.sh's job.
module Test.Diagnostics (tests) where

import Data.Char (isDigit)
import Data.List (nub, sort)
import qualified Data.Text as T
import Rue.Proto.Diagnostics
import Test.Tasty (TestTree, testGroup)
import Test.Tasty.HUnit (assertBool, testCase, (@?=))

tests :: TestTree
tests =
  testGroup
    "diagnostics"
    [ testCase "every code is E followed by four digits" $
        assertBool "malformed code" (all wellFormed allCodes)
    , testCase "codes are distinct" $
        length (nub (map codeText allCodes)) @?= length allCodes
    , testCase "codes are in ascending order" $
        map codeText allCodes @?= sort (map codeText allCodes)
    , testCase "every code has a non-empty meaning" $
        assertBool "empty meaning" (all (not . T.null . meaning) allCodes)
    ]
  where
    wellFormed c = case T.unpack (codeText c) of
      'E' : ds -> length ds == 4 && all isDigit ds
      _ -> False
