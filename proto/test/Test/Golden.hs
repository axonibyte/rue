-- | Tier 2: every declared artifact matches its expected file byte for byte,
-- and no expected file exists that nothing declares.
module Test.Golden (tests) where

import Control.Monad (filterM, forM)
import qualified Data.ByteString as B
import Data.List (sort, (\\))
import Rue.Proto.Golden
import Rue.Proto.Tenants (artifacts)
import System.Directory (createDirectoryIfMissing, doesDirectoryExist, doesFileExist, listDirectory)
import System.Environment (lookupEnv)
import System.FilePath (makeRelative, takeDirectory, takeFileName, (</>))
import Test.Tasty (TestTree, testGroup)
import Test.Tasty.HUnit (assertFailure, testCase, (@?=))

tests :: IO TestTree
tests = do
  root <- repoRoot
  buildDir <- maybe (root </> "proto" </> "dist-newstyle") id <$> lookupEnv "RUE_BUILDDIR"
  pure $
    testGroup
      "golden"
      [ testGroup "artifacts" [artifactTest root buildDir a | a <- artifacts]
      , testCase "no orphan expected files" $ do
          found <- expectedFiles root
          let declared = sort (map artifactPath artifacts)
              orphans = found \\ declared
          orphans @?= []
      ]

artifactTest :: FilePath -> FilePath -> Artifact -> TestTree
artifactTest root buildDir a = testCase (artifactPath a) $ do
  bytes <- either (assertFailure . (("could not produce " <> artifactPath a <> ": ") <>)) pure (artifactBytes a)
  let path = artifactAbsolute root a
  exists <- doesFileExist path
  if not exists
    then assertFailure ("missing expected file " <> path <> "; regenerate with RUE_UPDATE_GOLDENS=1 cabal run rue-proto-goldens")
    else do
      expected <- B.readFile path
      case compareBytes expected bytes of
        Nothing -> pure ()
        Just m -> do
          let actual = actualPath buildDir a
          createDirectoryIfMissing True (takeDirectory actual)
          B.writeFile actual bytes
          assertFailure (renderMismatch path actual m)

-- | Every file under an @expected@ directory beneath tenants/, plus the
-- generated transition table, as paths relative to the root.
expectedFiles :: FilePath -> IO [FilePath]
expectedFiles root = do
  tenantsDir <- doesDirectoryExist (root </> "tenants")
  under <- if tenantsDir then walk (root </> "tenants") else pure []
  let expected = [f | f <- under, "expected" `elem` splitDirs (takeDirectory f)]
  table <- filterM doesFileExist [root </> "docs" </> "state-transitions.tsv"]
  pure (sort (map (makeRelative root) (expected <> table)))
  where
    splitDirs p = go p []
      where
        go d acc
          | takeDirectory d == d = takeFileName d : acc
          | otherwise = go (takeDirectory d) (takeFileName d : acc)
    walk d = do
      names <- listDirectory d
      paths <- forM names $ \n -> do
        let p = d </> n
        isDir <- doesDirectoryExist p
        if isDir then walk p else pure [p]
      pure (concat paths)
