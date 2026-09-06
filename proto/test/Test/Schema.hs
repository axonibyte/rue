{-# LANGUAGE OverloadedStrings #-}
-- | Tier 3 in Haskell: docs/verdict-schema.json against the goldens.
--
-- A minimal validator supporting exactly the keywords the schema uses --
-- type (including a list with "null"), properties, required,
-- additionalProperties (false or a schema), items, enum, anyOf -- and
-- refusing any other keyword, so an unsupported construct fails rather than
-- passes. Asserts: every verdict golden validates; every property path the
-- schema declares is produced by at least one golden (a field nothing
-- produces is a claim the verdict does not make); verdict_version agrees.
module Test.Schema (tests) where

import Control.Monad (forM_)
import Data.Aeson (Value (..), decodeStrict)
import qualified Data.Aeson.Key as Key
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import Data.Foldable (toList)
import Data.List (isSuffixOf, nub, sort, (\\))
import Data.Scientific (isInteger)
import Data.Text (Text)
import qualified Data.Text as T
import qualified Data.Vector as V
import Rue.Proto.Golden (Artifact (..), repoRoot)
import Rue.Proto.Tenants (artifacts)
import Rue.Proto.Verdict (verdictVersion)
import System.FilePath ((</>))
import Test.Tasty (TestTree, testGroup)
import Test.Tasty.HUnit (assertBool, assertFailure, testCase, (@?=))

tests :: IO TestTree
tests = do
  root <- repoRoot
  schemaBytes <- B.readFile (root </> "docs" </> "verdict-schema.json")
  schema <- maybe (fail "docs/verdict-schema.json does not parse") pure (decodeStrict schemaBytes)
  let goldens = [(artifactPath a, v) | a <- artifacts, "verdict.json" `isSuffixOf` artifactPath a, Right bytes <- [artifactBytes a], Just v <- [decodeStrict bytes]]
  pure $
    testGroup
      "schema"
      [ testCase "the schema uses only supported keywords" $
          case unsupported schema of
            [] -> pure ()
            ks -> assertFailure ("unsupported schema keywords: " <> show ks)
      , testCase "every verdict golden validates" $
          forM_ goldens $ \(path, v) -> case validate schema v of
            [] -> pure ()
            errs -> assertFailure (path <> ":\n" <> unlines (map ("  " <>) errs))
      , testCase "every schema property path is produced by at least one golden" $ do
          let declared = sort (nub (schemaPaths [] schema))
              produced = sort (nub (concatMap (valuePaths [] . snd) goldens))
              missing = declared \\ produced
          missing @?= []
      , testCase "verdict_version agrees" $
          forM_ goldens $ \(path, v) -> case v of
            Object o | Just (Number n) <- KM.lookup "verdict_version" o -> assertBool path (n == fromIntegral verdictVersion)
            _ -> assertFailure (path <> ": no verdict_version")
      , testCase "there are verdict goldens to validate" $
          assertBool "no verdict goldens" (not (null goldens))
      ]

supportedKeywords :: [Text]
supportedKeywords = ["$schema", "$id", "title", "description", "type", "properties", "required", "additionalProperties", "items", "enum", "anyOf"]

unsupported :: Value -> [Text]
unsupported v = case v of
  Object o ->
    [k | k <- map Key.toText (KM.keys o), k `notElem` supportedKeywords]
      <> concatMap unsupported (subschemas o)
  _ -> []

subschemas :: KM.KeyMap Value -> [Value]
subschemas o =
  concat
    [ maybe [] (\p -> case p of Object ps -> toList ps; _ -> []) (KM.lookup "properties" o)
    , maybe [] (\a -> case a of Object _ -> [a]; _ -> []) (KM.lookup "additionalProperties" o)
    , maybe [] pure (KM.lookup "items" o)
    , maybe [] (\a -> case a of Array xs -> toList xs; _ -> []) (KM.lookup "anyOf" o)
    ]

-- | Errors, or none.
validate :: Value -> Value -> [String]
validate schema v = case schema of
  Object o -> concat [typeCheck o, enumCheck o, propsCheck o, itemsCheck o, anyOfCheck o]
  _ -> ["schema is not an object"]
  where
    typeCheck o = case KM.lookup "type" o of
      Nothing -> []
      Just t ->
        let allowed = case t of
              String s -> [s]
              Array xs -> [s | String s <- toList xs]
              _ -> []
         in if any (matchesType v) allowed then [] else ["type " <> show allowed <> " does not admit " <> summary v]
    enumCheck o = case KM.lookup "enum" o of
      Just (Array xs) | v `notElem` toList xs -> ["value " <> summary v <> " not in enum"]
      _ -> []
    propsCheck o = case v of
      Object obj ->
        let props = case KM.lookup "properties" o of
              Just (Object ps) -> ps
              _ -> KM.empty
            required = case KM.lookup "required" o of
              Just (Array xs) -> [Key.fromText s | String s <- toList xs]
              _ -> []
            missing = [Key.toText k | k <- required, not (KM.member k obj)]
            extra = [Key.toText k | k <- KM.keys obj, not (KM.member k props)]
            additional = KM.lookup "additionalProperties" o
            extraErrs = case additional of
              Just (Bool False) -> ["unexpected properties " <> show extra | not (null extra)]
              Just s@(Object _) -> concat [map ((T.unpack (Key.toText k) <> ": ") <>) (validate s x) | (k, x) <- KM.toList obj, not (KM.member k props)]
              _ -> []
            propErrs = concat [map ((T.unpack (Key.toText k) <> ": ") <>) (validate s x) | (k, x) <- KM.toList obj, Just s <- [KM.lookup k props]]
         in ["missing required " <> show missing | not (null missing)] <> extraErrs <> propErrs
      _ -> []
    itemsCheck o = case (KM.lookup "items" o, v) of
      (Just s, Array xs) -> concat [map (("[" <> show i <> "]: ") <>) (validate s x) | (i, x) <- zip [0 :: Int ..] (toList xs)]
      _ -> []
    anyOfCheck o = case KM.lookup "anyOf" o of
      Just (Array alts) | not (any (null . (`validate` v)) (toList alts)) -> ["no anyOf alternative admits " <> summary v]
      _ -> []

matchesType :: Value -> Text -> Bool
matchesType v t = case (t, v) of
  ("object", Object _) -> True
  ("array", Array _) -> True
  ("string", String _) -> True
  ("boolean", Bool _) -> True
  ("null", Null) -> True
  ("integer", Number n) -> isInteger n
  ("number", Number _) -> True
  _ -> False

summary :: Value -> String
summary v = case v of
  Object _ -> "an object"
  Array _ -> "an array"
  String s -> show s
  Number n -> show n
  Bool b -> show b
  Null -> "null"

-- | Every property path the schema declares, arrays as "[]", map-valued
-- objects (additionalProperties as a schema) as "{}".
schemaPaths :: [Text] -> Value -> [Text]
schemaPaths prefix v = case v of
  Object o ->
    let here = [T.intercalate "." (reverse prefix) | not (null prefix)]
        props = case KM.lookup "properties" o of
          Just (Object ps) -> concat [schemaPaths (Key.toText k : prefix) s | (k, s) <- KM.toList ps]
          _ -> []
        additional = case KM.lookup "additionalProperties" o of
          Just s@(Object _) -> schemaPaths ("{}" : prefix) s
          _ -> []
        items = case KM.lookup "items" o of
          Just s -> schemaPaths ("[]" : prefix) s
          Nothing -> []
        alts = case KM.lookup "anyOf" o of
          Just (Array xs) -> concatMap (schemaPaths prefix) (toList xs)
          _ -> []
     in here <> props <> additional <> items <> alts
  _ -> []

-- | Every path a value populates with a non-null, and for objects and arrays
-- any, value. Keys under a map-valued object are collapsed to "{}" when the
-- schema declares no properties for them; here the two map-valued fields
-- (hosts_touched, drift_policy) are known by name.
valuePaths :: [Text] -> Value -> [Text]
valuePaths prefix v =
  let here = [T.intercalate "." (reverse prefix) | not (null prefix), v /= Null]
   in here <> case v of
        Object o -> concat [valuePaths (keyOf k : prefix) x | (k, x) <- KM.toList o]
        Array xs -> concatMap (valuePaths ("[]" : prefix)) (V.toList xs)
        _ -> []
  where
    keyOf k = if mapValued then "{}" else Key.toText k
    mapValued = case prefix of
      "hosts_touched" : _ -> True
      "drift_policy" : _ -> True
      _ -> False
