-- | Print a tenant case's verdict: @rue-proto-check <tenant> <host>
-- [--json | --prose | --explain]@. The default is the prose verdict, which is
-- the last line printed, as every rue verb that acts on the world ends in one.
module Main (main) where

import qualified Data.ByteString as B
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import Rue.Proto.Check (check, deferredSteps)
import Rue.Proto.Explain (explain)
import qualified Rue.Proto.Json.Canonical as Canonical
import Rue.Proto.Prose (prose)
import Rue.Proto.Tenants (lookupCase)
import Rue.Proto.Verdict (Status (..), toJson, vStatus)
import System.Environment (getArgs)
import System.Exit (ExitCode (..), exitWith)
import System.IO (hPutStrLn, hSetBinaryMode, stderr, stdout)

main :: IO ()
main = do
  hSetBinaryMode stdout True
  args <- getArgs
  case args of
    [tenant, host] -> run tenant host "--prose"
    [tenant, host, mode] | mode `elem` ["--json", "--prose", "--explain"] -> run tenant host mode
    _ -> usage
  where
    usage = do
      hPutStrLn stderr "usage: rue-proto-check <tenant> <host> [--json | --prose | --explain]"
      exitWith (ExitFailure 2)
    run tenant host mode = case lookupCase (T.pack tenant) (T.pack host) of
      Nothing -> do
        hPutStrLn stderr ("rue-proto-check: no case " <> tenant <> "/" <> host)
        exitWith (ExitFailure 2)
      Just (site, requester, plan) -> do
        let v = check site requester plan
        case mode of
          "--json" -> either (\e -> hPutStrLn stderr e >> exitWith (ExitFailure 2)) (B.hPut stdout) (Canonical.encode (toJson v))
          "--explain" -> B.hPut stdout (TE.encodeUtf8 (explain plan (deferredSteps site plan)))
          _ -> B.hPut stdout (TE.encodeUtf8 (prose v))
        exitWith (if vStatus v == Ok then ExitSuccess else ExitFailure 1)
