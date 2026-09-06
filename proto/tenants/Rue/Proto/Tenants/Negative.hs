{-# LANGUAGE OverloadedStrings #-}
-- | The negative cases: plans the checker must refuse with exactly one named
-- code, each a golden under @tenants/_negative/@.
--
-- The first twelve are docs/ROADMAP.md Phase 0 task 8, derived from T1 and
-- T3 by one change each. The rest cover every other code the Phase 0 checker
-- emits, so that the set of negative goldens is exactly the set of emitted
-- codes (Test.Tenants binds the two).
module Rue.Proto.Tenants.Negative (cases, lab) where

import Rue.Proto.Diagnostics (Code (..))
import Rue.Proto.Model
import Rue.Proto.Tenants.Common
import qualified Rue.Proto.Tenants.T1 as T1
import qualified Rue.Proto.Tenants.T2 as T2
import qualified Rue.Proto.Tenants.T3 as T3

neg :: Code -> Host -> Site -> Plan -> NegativeCase
neg c slug st p = NegativeCase c slug st "requester" p

-- | A small site for the cases no tenant motivates: a filesystem host with
-- ssh, an API appliance with no filesystem, and a host reachable only by
-- console; no max_wait; a scheduler on db-01 only.
lab :: Site
lab =
  Site
    { siteHosts =
        [ HostRecord "db-01" "freebsd" ["ssh"] True
        , HostRecord "api-01" "appliance" ["api"] False
        , HostRecord "island" "freebsd" ["console"] True
        ]
    , siteTransports = ["ssh"]
    , siteAuthenticators = [Authenticator "oncall" True, Authenticator "alice" True, Authenticator "driver" False]
    , siteMaxWait = Nothing
    , siteSchedulerPresent = ["db-01"]
    }

-- | A one-hour temporary plan on db-01.
temp :: Host -> [Item] -> Plan
temp name items = (plan name "db-01" items) {planWane = Just (Duration 3600)}

owned :: Host -> Op
owned f = op f [entry Owned ("file:/etc/" <> f)]

s :: Op -> Item
s = Step . step

targetUndo :: Op -> Op
targetUndo o = o {opUndoLocus = UndoTarget}

after1h :: Backstop
after1h = Backstop [After (Duration 3600)] (ArmBefore 1)

cases :: [NegativeCase]
cases =
  [ -- docs/ROADMAP.md Phase 0 task 8, in its order.
    -- T3 with the backstop armed after the change: the classic self-lockout.
    neg E0401 "backstop-armed-after-reach" T3.site (T3.openMgmtPort {planBackstop = Just (Backstop [UnlessConfirmed (Duration 600)] (ArmBefore 2))})
  , -- T3 with the undo locus moved to the controller.
    neg E0401 "reach-with-controller-undo" T3.site (T3.openMgmtPort {planBody = [Step (step T3.pfAllow {opUndoLocus = UndoController}), Observe "verify_reach" "reach", Confirm, Commit]})
  , -- An auto plan with a force: on a step.
    neg E0404 "auto-with-force" T1.site ((T1.breakglass {planMode = Auto, planGate = Nothing}) {planBody = [Step (step (op "posture" [entry Owned "file:/etc/x"])) {stepForce = [ForceUnknown]}]})
  , -- T3 with a modified footprint under reach, drift defaulting to :defer.
    neg E0410 "reach-with-defer" T3.site (T3.openMgmtPort {planBody = [Step (step T3.pfAllow {opFootprint = [entry Modified "file:/etc/pf.conf"]}), Observe "verify_reach" "reach", Confirm, Commit]})
  , -- Both wane and commit().
    neg E0501 "wane-and-commit" T3.site (T3.openMgmtPort {planWane = Just (Duration 3600)})
  , -- Neither wane nor commit().
    neg E0501 "neither-wane-nor-commit" T1.site (T1.breakglass {planWane = Nothing, planBackstop = Nothing, planGate = Nothing})
  , -- commit() not last on its path.
    neg E0502 "commit-not-last" T3.site (T3.openMgmtPort {planBody = [Step (step T3.pfAllow), Commit, Observe "verify_reach" "reach", Confirm]})
  , -- A permanent plan whose else arm never commits.
    neg E0505 "path-without-commit" T3.site (T3.openMgmtPort {planBody = [Step (step T3.pfAllow), Observe "verify_reach" "reach", Confirm, When (guard "healthy" Yes) Nothing LapseRevert [Commit] []]})
  , -- A permanent plan whose step gate has no window and whose site has no max_wait.
    neg E0506 "unbounded-wait" T3.site (T3.openMgmtPort {planBody = [Step (step T3.pfAllow) {stepGate = Just (Single (Auth "netops" 1))}, Observe "verify_reach" "reach", Confirm, Commit]})
  , -- An auto plan whose knell wants a human acknowledgement (T2's site, so the wait itself is bounded by max_wait).
    neg E0507 "auto-with-human-ack" T2.site ((plan "promote" "node-b" [KnellItem (step ((op "fence" []) {opUndo = NoUndo, opUndoLocus = UndoNone, opRefusal = Knell (Just (guard "verified_off" Yes)) (CostProbe "fence_verdict") (AckGate (Single (Humans 1)))})), Commit]) {planMode = Auto})
  , -- A gate that counts the requester.
    neg E0508 "gate-counts-requester" (T1.site {siteAuthenticators = siteAuthenticators T1.site <> [Authenticator "requester" True]}) (T1.breakglass {planGate = Just (PlanGate (Single (Auth "requester" 1)) (Just (Duration 1800)) False)})
  , -- A gate satisfiable by waiting alone.
    neg E0509 "zero-human-gate" T1.site (T1.breakglass {planGate = Just (PlanGate (Single (Wait (Duration 1800) 1)) (Just (Duration 3600)) False)})
  , -- A windowed step gate on a knell, satisfiable by waiting alone.
    neg E0509 "zero-human-step-gate" lab (temp "fence" [KnellItem (step ((op "fence" []) {opUndo = NoUndo, opUndoLocus = UndoNone, opRefusal = Knell (Just (guard "verified_off" Yes)) (CostProbe "fence_verdict") (AckNone "driver verified off")})) {stepGate = Just (Single (Wait (Duration 1800) 1)), stepWindow = Just (Duration 3600)}])
  , -- Op rules (section 5.3).
    neg E0201 "no-undo-not-knell" lab (temp "posture" [s (owned "a") {opUndo = NoUndo}])
  , neg E0202 "target-undo-not-closed" lab (temp "posture" [s (targetUndo (owned "a")) {opUndoClosed = False}])
  , neg E0203 "none-locus-with-undo" lab (temp "posture" [s (owned "a") {opUndoLocus = UndoNone}])
  , neg E0205 "held-without-suspend" lab (temp "tunnel" [s (op "tunnel" [entry Held "proc:tunnel"])])
  , neg E0207 "compensate-without-undo-pre" lab (temp "posture" [s (owned "a") {opUndo = Compensate []}])
  , neg E0208 "undo-not-idempotent" lab (temp "posture" [s (owned "a") {opUndoIdempotent = False}])
  , neg E0407 "target-undo-on-api-host" lab (temp "bmc" [s (targetUndo (owned "a")) {opLocus = HostLocus (StaticHost "api-01")}])
  , -- Interference (section 5.7).
    neg E0301 "umbra-conflict" lab (temp "twice" [s (owned "a"), s (owned "a")])
  , neg E0302 "may-conflict-strict" lab (temp "any" [s (owned "x"), s (op "any" [entry Owned "file:/etc/{name}"])])
  , -- The same shape on a host bound at runtime: penumbral by host, so a may-conflict (and an unresolved binding in the verdict).
    neg E0302 "may-conflict-bound-host" lab (temp "any" [s (owned "a"), s (owned "a") {opLocus = HostLocus (BoundHost "pick"), opUndoLocus = UndoController}])
  , neg E0303 "par-not-disjoint" lab (temp "par" [Par [s (owned "a"), s (owned "a")]])
  , neg E0304 "reach-inside-par" lab ((temp "par" [Par [s (targetUndo (owned "pf")) {opReach = ["ssh"]}, s (owned "b")]]) {planBackstop = Just after1h})
  , neg E0305 "anchor-twice" lab (temp "regions" [s (op "r1" [anchored "file:/etc/keys" "rue"]), s (op "r2" [anchored "file:/etc/keys" "rue"])])
  , -- Backstops (section 5.6).
    neg E0403 "scheduler-absent" lab ((temp "posture" [s (targetUndo (owned "a"))]) {planBackstop = Just after1h, planOwner = "island"})
  , neg E0405 "heartbeat-too-slow" lab ((temp "posture" [s (targetUndo (owned "a"))]) {planBackstop = Just (Backstop [After (Duration 3600), UnlessHeartbeat (Duration 60) (Just (Duration 30))] (ArmBefore 1))})
  , neg E0503 "backstop-after-not-wane" lab ((temp "posture" [s (targetUndo (owned "a"))]) {planBackstop = Just (Backstop [After (Duration 7200)] (ArmBefore 1))})
  , neg E0504 "permanent-backstop-on-timer" lab ((plan "posture" "db-01" [s (targetUndo (owned "a")), Commit]) {planBackstop = Just after1h})
  ]
