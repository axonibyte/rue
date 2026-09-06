-- | Where goldens live and how they are compared.
--
-- The test-suite and the @rue-proto-goldens@ writer share this module so they
-- cannot disagree about the path scheme. The test-suite is read-only; the
-- writer is the only thing that writes, and only when told to.
module Rue.Proto.Golden
  ( Artifact (..)
  , repoRoot
  , artifactAbsolute
  , actualPath
  , compareBytes
  , Mismatch (..)
  , renderMismatch
  ) where

import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import System.Directory (doesFileExist, getCurrentDirectory)
import System.Environment (lookupEnv)
import System.FilePath (takeDirectory, (</>))

-- | One golden: a path relative to the repository root and the bytes that
-- must be there, or the reason they could not be produced.
data Artifact = Artifact
  { artifactPath :: FilePath
  , artifactBytes :: Either String B.ByteString
  }

-- | The repository root: @RUE_REPO_ROOT@ if set, otherwise the nearest
-- ancestor of the working directory that contains @.reaper.toml@.
repoRoot :: IO FilePath
repoRoot = do
  env <- lookupEnv "RUE_REPO_ROOT"
  case env of
    Just r | not (null r) -> pure r
    _ -> getCurrentDirectory >>= walk
  where
    walk d = do
      here <- doesFileExist (d </> ".reaper.toml")
      if here
        then pure d
        else do
          let up = takeDirectory d
          if up == d
            then fail "repoRoot: no .reaper.toml in any ancestor of the working directory, and RUE_REPO_ROOT is unset"
            else walk up

artifactAbsolute :: FilePath -> Artifact -> FilePath
artifactAbsolute root a = root </> artifactPath a

-- | Where a failing comparison leaves the bytes it actually produced, so a
-- human can diff them against the expected file.
actualPath :: FilePath -> Artifact -> FilePath
actualPath buildDir a = buildDir </> "golden-actual" </> artifactPath a

data Mismatch = Mismatch
  { mismatchLine :: Int
  , expectedContext :: [B.ByteString]
  , actualContext :: [B.ByteString]
  , expectedLength :: Int
  , actualLength :: Int
  }

-- | 'Nothing' when equal; otherwise the first differing line with context.
compareBytes :: B.ByteString -> B.ByteString -> Maybe Mismatch
compareBytes expected actual
  | expected == actual = Nothing
  | otherwise =
      let el = BC.lines expected
          al = BC.lines actual
          firstDiff = length (takeWhile id (zipWith (==) el al))
          ctx xs = take 5 (drop (max 0 (firstDiff - 2)) xs)
       in Just
            Mismatch
              { mismatchLine = firstDiff + 1
              , expectedContext = ctx el
              , actualContext = ctx al
              , expectedLength = B.length expected
              , actualLength = B.length actual
              }

renderMismatch :: FilePath -> FilePath -> Mismatch -> String
renderMismatch expectedFile actualFile m =
  unlines $
    [ "golden mismatch at line " <> show (mismatchLine m)
    , "  expected (" <> show (expectedLength m) <> " bytes): " <> expectedFile
    , "  actual   (" <> show (actualLength m) <> " bytes): " <> actualFile
    , "  --- expected, around the difference:"
    ]
      <> map (("    " <>) . BC.unpack) (expectedContext m)
      <> ["  --- actual, around the difference:"]
      <> map (("    " <>) . BC.unpack) (actualContext m)
      <> ["  regenerate with: RUE_UPDATE_GOLDENS=1 cabal run rue-proto-goldens"]
