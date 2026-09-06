-- | Print the runtime state transition table, generated from the five class
-- rules of docs/ROADMAP.md section 5.9. docs/state-transitions.tsv is this
-- output, golden-tested.
module Main (main) where

import qualified Data.ByteString as B
import qualified Data.Text.Encoding as TE
import Rue.Proto.States (renderTable)
import System.IO (hSetBinaryMode, stdout)

main :: IO ()
main = do
  hSetBinaryMode stdout True
  B.hPut stdout (TE.encodeUtf8 renderTable)
