-- | The canonical JSON printer: exact bytes on fixed cases, and a round trip
-- on generated values (encode, parse with aeson, re-encode: same bytes; and
-- the parse is the original value).
module Test.Canonical (tests) where

import Data.Aeson (Value (..), decodeStrict)
import qualified Data.Aeson.Key as Key
import qualified Data.Aeson.KeyMap as KeyMap
import qualified Data.ByteString.Char8 as BC
import qualified Data.Text as T
import qualified Data.Vector as V
import Rue.Proto.Json.Canonical (encode)
import Test.Tasty (TestTree, testGroup)
import Test.Tasty.HUnit (testCase, (@?=))
import Test.Tasty.QuickCheck

tests :: TestTree
tests =
  testGroup
    "canonical-json"
    [ testCase "empty object" $ encode (Object mempty) @?= Right (BC.pack "{}\n")
    , testCase "empty array" $ encode (Array mempty) @?= Right (BC.pack "[]\n")
    , testCase "keys sorted by code point, two-space indent" $
        encode (obj [("b", Number 2), ("a", Number 1), ("Z", Bool True)])
          @?= Right (BC.pack "{\n  \"Z\": true,\n  \"a\": 1,\n  \"b\": 2\n}\n")
    , testCase "nested containers, one element per line" $
        encode (obj [("xs", Array (V.fromList [Number 1, obj [("k", Null)]]))])
          @?= Right (BC.pack "{\n  \"xs\": [\n    1,\n    {\n      \"k\": null\n    }\n  ]\n}\n")
    , testCase "escaping: quote, backslash, named controls, other controls, raw non-ASCII" $
        encode (String (T.pack "a\"b\\c\nd\t\SOH\233\x2013"))
          @?= Right expectedEscaped
    , testCase "non-integer numbers are refused" $
        case encode (Number 1.5) of
          Left _ -> pure ()
          Right b -> fail ("accepted a float: " <> BC.unpack b)
    , testProperty "round trip: parse . encode = id, and encode . parse . encode = encode" $
        forAll genValue $ \v -> case encode v of
          Left e -> counterexample e False
          Right bytes -> case decodeStrict bytes of
            Nothing -> counterexample ("unparseable: " <> BC.unpack bytes) False
            Just v' -> v' === v .&&. encode v' === Right bytes
    ]
  where
    obj = Object . KeyMap.fromList . map (\(k, x) -> (Key.fromString k, x))
    -- U+00E9 and U+2013 stay raw UTF-8; the expected bytes are written out
    -- explicitly so the test does not compute them with the code under test.
    expectedEscaped = BC.pack "\"a\\\"b\\\\c\\nd\\t\\u0001" <> BC.pack "\xC3\xA9\xE2\x80\x93" <> BC.pack "\"\n"

-- | Values with integer numbers only, modest depth, and keys drawn from a
-- set that exercises code-point ordering (upper before lower, digits first).
genValue :: Gen Value
genValue = sized go
  where
    -- Fan-out is bounded per level, or the generator's size blows up
    -- exponentially with depth (it did: the first run never finished).
    go n
      | n <= 0 = leaf
      | otherwise =
          frequency
            [ (3, leaf)
            , (1, Array . V.fromList <$> few (go (n `div` 3)))
            , (2, obj <$> few ((,) <$> genKey <*> go (n `div` 3)))
            ]
    few g = choose (0, 4 :: Int) >>= \k -> vectorOf k g
    leaf =
      oneof
        [ pure Null
        , Bool <$> arbitrary
        , Number . fromInteger <$> arbitrary
        , String . T.pack <$> listOf genChar
        ]
    genKey = T.pack <$> listOf1 (elements "09AZaz_-.")
    genChar = frequency [(8, elements (['a' .. 'z'] ++ "\"\\ ")), (1, elements "\n\r\t\b\f\SOH\US"), (1, elements "\233\x2013\x1F600")]
    obj = Object . KeyMap.fromList . map (\(k, x) -> (Key.fromText k, x))
