-- | Regenerate the expected files under tenants/ and docs/.
--
-- The only writer of goldens. Refuses unless RUE_UPDATE_GOLDENS=1, which CI
-- and reaper never set; the test-suite is read-only and tools/check.sh also
-- fails if a test run changes anything under tenants/ or docs/.
module Main (main) where

import Control.Monad (forM, unless)
import qualified Data.ByteString as B
import Rue.Proto.Golden
import Rue.Proto.Tenants (artifacts)
import System.Directory (createDirectoryIfMissing, doesFileExist)
import System.Environment (lookupEnv)
import System.Exit (exitWith, ExitCode (..))
import System.FilePath (takeDirectory)
import System.IO (hPutStrLn, stderr)

main :: IO ()
main = do
  flag <- lookupEnv "RUE_UPDATE_GOLDENS"
  unless (flag == Just "1") $ do
    hPutStrLn stderr "rue-proto-goldens: refusing to write; set RUE_UPDATE_GOLDENS=1 to regenerate goldens deliberately"
    exitWith (ExitFailure 2)
  root <- repoRoot
  results <- forM artifacts $ \a -> case artifactBytes a of
    Left err -> do
      hPutStrLn stderr ("rue-proto-goldens: " <> artifactPath a <> ": " <> err)
      pure False
    Right bytes -> do
      let path = artifactAbsolute root a
      exists <- doesFileExist path
      status <-
        if exists
          then do
            old <- B.readFile path
            pure (if old == bytes then "unchanged" else "changed")
          else pure "new"
      createDirectoryIfMissing True (takeDirectory path)
      B.writeFile path bytes
      putStrLn (status <> "  " <> artifactPath a)
      pure True
  unless (and results) $ exitWith (ExitFailure 1)
  putStrLn ("rue-proto-goldens: " <> show (length artifacts) <> " artifacts written")
