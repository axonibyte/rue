-- | Canonical JSON: the byte format a later implementation must reproduce.
--
-- Specified in docs/TESTING.md and implemented once, here, so the format is
-- ours rather than a third-party pretty-printer's:
--
--   * UTF-8; object keys sorted by code point; two-space indent;
--     @"key": value@; every array or object element on its own line; empty
--     containers as @[]@ and @{}@; one trailing LF; no trailing whitespace.
--   * Numbers are integers only. A non-integer is refused with 'Left' so a
--     golden can never depend on a float format.
--   * Strings escape @"@, @\\@ and controls below 0x20 (@\\n \\r \\t \\b \\f@,
--     else @\\u00xx@ in lowercase hex); everything else is raw UTF-8.
--
-- This is byte-compatible with @serde_json::to_string_pretty@ plus a trailing
-- newline, which is what "after canonicalisation" means in Phase 1.
module Rue.Proto.Json.Canonical
  ( encode
  ) where

import Data.Aeson (Value (..))
import qualified Data.Aeson.Key as Key
import qualified Data.Aeson.KeyMap as KeyMap
import qualified Data.ByteString as B
import qualified Data.ByteString.Builder as BB
import qualified Data.ByteString.Lazy as BL
import Data.Char (ord)
import Data.Foldable (toList)
import Data.List (sortOn)
import Data.Scientific (floatingOrInteger)
import Data.Text (Text)
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import Numeric (showHex)

-- | Encode a value canonically, or explain why it cannot be.
encode :: Value -> Either String B.ByteString
encode v = do
  b <- go 0 v
  pure (BL.toStrict (BB.toLazyByteString (b <> BB.char7 '\n')))

go :: Int -> Value -> Either String BB.Builder
go _ Null = Right (BB.string7 "null")
go _ (Bool True) = Right (BB.string7 "true")
go _ (Bool False) = Right (BB.string7 "false")
go _ (Number n) = case floatingOrInteger n :: Either Double Integer of
  Right i -> Right (BB.integerDec i)
  Left _ -> Left ("canonical JSON admits integers only; refusing " <> show n)
go _ (String s) = Right (string s)
go depth (Array xs)
  | null xs = Right (BB.string7 "[]")
  | otherwise = do
      items <- traverse (go (depth + 1)) (toList xs)
      pure (container depth '[' ']' items)
go depth (Object o)
  | KeyMap.null o = Right (BB.string7 "{}")
  | otherwise = do
      let pairs = sortOn fst [(Key.toText k, x) | (k, x) <- KeyMap.toList o]
      items <- traverse (\(k, x) -> (\b -> string k <> BB.string7 ": " <> b) <$> go (depth + 1) x) pairs
      pure (container depth '{' '}' items)

container :: Int -> Char -> Char -> [BB.Builder] -> BB.Builder
container depth open close items =
  BB.char7 open
    <> BB.char7 '\n'
    <> mconcat (zipWith line [0 :: Int ..] items)
    <> indent depth
    <> BB.char7 close
  where
    n = length items
    line i b = indent (depth + 1) <> b <> (if i == n - 1 then mempty else BB.char7 ',') <> BB.char7 '\n'

indent :: Int -> BB.Builder
indent d = BB.byteString (B.replicate (2 * d) 0x20)

string :: Text -> BB.Builder
string s = BB.char7 '"' <> T.foldr (\c acc -> escape c <> acc) mempty s <> BB.char7 '"'
  where
    escape c = case c of
      '"' -> BB.string7 "\\\""
      '\\' -> BB.string7 "\\\\"
      '\n' -> BB.string7 "\\n"
      '\r' -> BB.string7 "\\r"
      '\t' -> BB.string7 "\\t"
      '\b' -> BB.string7 "\\b"
      '\f' -> BB.string7 "\\f"
      _
        | ord c < 0x20 -> BB.string7 ("\\u" <> pad4 (showHex (ord c) ""))
        | otherwise -> BB.byteString (TE.encodeUtf8 (T.singleton c))
    pad4 h = replicate (4 - length h) '0' <> h
