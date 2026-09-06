-- | Print a tenant's verdict to stdout. Filled in once the checker exists.
module Main (main) where

import Rue.Proto.Tenants (artifacts)
import System.Exit (exitWith, ExitCode (..))
import System.IO (hPutStrLn, stderr)

main :: IO ()
main = do
  hPutStrLn stderr ("rue-proto-check: the checker is not implemented yet (" <> show (length artifacts) <> " artifacts declared)")
  exitWith (ExitFailure 2)
