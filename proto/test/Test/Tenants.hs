{-# LANGUAGE OverloadedStrings #-}
-- | Tier 2, the Phase 0 acceptance (docs/ROADMAP.md section 9): every tenant
-- expresses and checks clean; every negative case refuses with exactly its
-- code; the negative goldens cover exactly the codes the checker emits; and
-- the claims section 8 makes about each verdict hold as fields, not only as
-- prose bytes. Also the source-as-data half: every case directory carries
-- its @.rue@ text (unparsed in Phase 0) and every tenant an inventory.
module Test.Tenants (tests) where

import Control.Monad (forM_)
import Data.List (nub, sort)
import qualified Data.Text as T
import Rue.Proto.Check (check)
import Rue.Proto.Intent (Intent (..))
import Rue.Proto.Diagnostics (codeText)
import Rue.Proto.Golden (repoRoot)
import Rue.Proto.Model
import Rue.Proto.Tenants (negatives, tenants)
import Rue.Proto.Tenants.Common
import qualified Rue.Proto.Tenants.T1 as T1
import qualified Rue.Proto.Tenants.T2 as T2
import qualified Rue.Proto.Tenants.T3 as T3
import qualified Rue.Proto.Tenants.T4 as T4
import Rue.Proto.Verdict
import System.Directory (doesFileExist)
import System.FilePath ((</>))
import Test.Check (emittedCodes)
import Test.Tasty (TestTree, testGroup)
import Test.Tasty.HUnit (assertBool, assertFailure, testCase, (@?=))

tests :: IO TestTree
tests = do
  root <- repoRoot
  pure $
    testGroup
      "tenants"
      [ testGroup "every tenant checks clean" [testCase (T.unpack (tenantName t <> "/" <> caseHost c)) (clean t c) | t <- tenants, c <- tenantCases t]
      , testGroup "every negative refuses with exactly its code" [testCase (negDir n) (refusesWith n) | n <- negatives]
      , testCase "the negative goldens cover exactly the emitted codes" $
          sort (nub (map negCode negatives)) @?= emittedCodes
      , testCase "negative directories are distinct" $
          let ds = map negDir negatives in sort (nub ds) @?= sort ds
      , testCase "case directories are distinct within a tenant" $
          forM_ tenants $ \t -> let hs = map caseHost (tenantCases t) in sort (nub hs) @?= sort hs
      , testGroup
          "the .rue text and inventory exist for every case"
          ( [testCase (T.unpack (tenantName t) <> "/" <> f) (exists (root </> "tenants" </> T.unpack (tenantName t) </> f)) | t <- tenants, f <- ["plan.rue", "inventory.toml"]]
              <> [testCase (negDir n <> "/plan.rue") (exists (root </> "tenants" </> "_negative" </> negDir n </> "plan.rue")) | n <- negatives]
          )
      , testGroup
          "T1 break-glass (section 8.1)"
          [ testCase "temporary, reverts at wane 4h, fully reversible" $ do
              vIntent t1 @?= Just Temporary
              vWane t1 @?= Just (Duration 14400)
              vReversibleThrough t1 @?= 4
              vPointOfNoReturn t1 @?= Nothing
          , testCase "backstop covers 1-2 on the target, installed before 1, armed after 2" $ do
              fmap bvCovers (vBackstop t1) @?= Just [1, 2]
              fmap bvInstalledBefore (vBackstop t1) @?= Just (Just 1)
              fmap bvArmedAfter (vBackstop t1) @?= Just (Just 2)
              fmap bvLateArmingWindow (vBackstop t1) @?= Just [1, 2]
          , testCase "steps 3-4 revert only while the engine lives" $ vControllerOnlyUndos t1 @?= [3, 4]
          , testCase "step 3 is on an API host with controller-side markers" $
              lookup 3 (vHostsTouched t1) @?= Just [HostTouched "bmc-01" "controller"]
          , testCase "the region step's fallback is conditional on no foreign region (task 8)" $ do
              map svConditional (vSteps t1) @?= [Nothing, Just "foreign region in file:/root/.ssh/authorized_keys", Nothing, Nothing]
              fmap (map fst . bvConditional) (vBackstop t1) @?= Just [2]
          , testCase "gate satisfiable by one human, windowed" $ do
              fmap gvSatisfiable (vGate t1) @?= Just True
              fmap gvMinDistinctHumans (vGate t1) @?= Just (Just 1)
              fmap gvWindow (vGate t1) @?= Just (Just (Duration 1800))
          ]
      , testGroup
          "T2 succession (section 8.2)"
          [ testCase "permanent, commits after the guests are up" $ do
              vIntent t2a @?= Just Permanent
              vCommitStep t2a @?= Just 9
              vCommitStep t2m @?= Just 10
          , testCase "reversible through the probes rung; the fence is the point of no return" $ do
              vReversibleThrough t2a @?= 3
              fmap ponrStep (vPointOfNoReturn t2a) @?= Just 4
              fmap ponrGuard (vPointOfNoReturn t2a) @?= Just (Just "fence_verified_off")
              fmap ponrCost (vPointOfNoReturn t2a) @?= Just "fence_verdict"
          , testCase "acknowledged by :none under auto and one human under manual" $ do
              fmap ponrAck (vPointOfNoReturn t2a) @?= Just "none"
              fmap ponrAck (vPointOfNoReturn t2m) @?= Just "humans()"
          , testCase "post-fence holds are held indefinitely; the heir is deferred with a handoff" $ do
              assertBool "no holds" (not (null (vHoldsAt t2a)))
              vHeldIndefinitely t2a @?= [5, 6, 8]
              vHeldIndefinitely t2m @?= [6, 7, 9]
              vDeferred t2a @?= [8]
              vDeferred t2m @?= [9]
          , testCase "the second knell is reachable only on the manual path" $ do
              length [() | sv <- vSteps t2a, svRefusal sv == "knell"] @?= 1
              length [() | sv <- vSteps t2m, svRefusal sv == "knell"] @?= 2
          , testCase "the per-guest loop checks clean under strict" $ do
              planStrictness T2.promoteAuto @?= Strict
              vMayConflicts t2a @?= []
          , testCase "the auto plan contains no human wait" $ do
              vMode t2a @?= "auto"
              vMode t2m @?= "manual"
              vGate t2a @?= Nothing
              map svGate (vSteps t2a) @?= replicate (length (vSteps t2a)) Nothing
              vInducedDefer t2a @?= [5, 6, 8]
          ]
      , testGroup
          "T3 commit-confirmed change (section 8.3)"
          [ testCase "permanent, commits at 4, reversible through 1" $ forM_ [t3, t3w] $ \v -> do
              vIntent v @?= Just Permanent
              vCommitStep v @?= Just 4
              vReversibleThrough v @?= 1
          , testCase "backstop covers step 1, installed and armed before it" $ forM_ [t3, t3w] $ \v -> do
              fmap bvCovers (vBackstop v) @?= Just [1]
              fmap bvInstalledBefore (vBackstop v) @?= Just (Just 1)
              fmap bvArmedBefore (vBackstop v) @?= Just (Just 1)
              fmap bvArmedAfter (vBackstop v) @?= Just Nothing
          , testCase "the region variant's revert is conditional; the Windows owned rule's is not" $ do
              map svConditional (vSteps t3) @?= [Just "foreign region in file:/etc/pf.conf", Nothing, Nothing, Nothing]
              map svConditional (vSteps t3w) @?= replicate 4 Nothing
          ]
      , testGroup
          "T4 reactive host (section 8.4)"
          [ testCase "temporary, reverts at wane, reversible through 1, no backstop" $ forM_ [t4, t4d] $ \v -> do
              vIntent v @?= Just Temporary
              vWane v @?= Just (Duration 7200)
              vReversibleThrough v @?= 1
              vBackstop v @?= Nothing
          , testCase "drift policy stated per step, clobber then defer" $ do
              map svDrift (vSteps t4) @?= [Just "clobber"]
              map svDrift (vSteps t4d) @?= [Just "defer"]
          , testCase "no instance directory: markers on the controller, undo only while the engine lives" $ do
              lookup 1 (vHostsTouched t4) @?= Just [HostTouched "site-ctl" "controller"]
              vControllerOnlyUndos t4 @?= [1]
          ]
      ]
  where
    verdict t c = check (tenantSite t) (tenantRequester t) (casePlan c)
    clean t c = do
      let v = verdict t c
      vDiagnostics v @?= []
      vStatus v @?= Ok
    refusesWith n = do
      let v = check (negSite n) (negRequester n) (negPlan n)
      vStatus v @?= RefusedStatus
      map diagCode (vDiagnostics v) @?= [negCode n]
    negDir n = T.unpack (codeText (negCode n) <> "-" <> negSlug n)
    exists path = do
      ok <- doesFileExist path
      if ok then pure () else assertFailure ("missing " <> path)
    t1 = check T1.site (tenantRequester T1.tenant) T1.breakglass
    t2a = check T2.site (tenantRequester T2.tenant) T2.promoteAuto
    t2m = check T2.site (tenantRequester T2.tenant) T2.promoteManual
    t3 = check T3.site (tenantRequester T3.tenant) T3.openMgmtPort
    t3w = check T3.site (tenantRequester T3.tenant) T3.openMgmtPortWindows
    t4 = check T4.site (tenantRequester T4.tenant) T4.shedLoad
    t4d = check T4.site (tenantRequester T4.tenant) T4.shedLoadDeferring
