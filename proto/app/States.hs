-- | Print the runtime state transition table. Filled in once the state
-- machine exists.
module Main (main) where

import System.Exit (exitWith, ExitCode (..))
import System.IO (hPutStrLn, stderr)

main :: IO ()
main = do
  hPutStrLn stderr "rue-proto-states: the state machine is not implemented yet"
  exitWith (ExitFailure 2)
