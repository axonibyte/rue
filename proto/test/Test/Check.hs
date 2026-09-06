{-# LANGUAGE OverloadedStrings #-}
-- | Tier 1 for the checker: every code the prototype can raise has a plan
-- that raises it and a sibling that does not, and the codes it can raise are
-- stated as a list so the not-proven table is a fact, not a guess.
module Test.Check (tests, emittedCodes) where

import Data.List (sort)
import Data.Text (Text)
import Rue.Proto.Check
import Rue.Proto.Diagnostics (Code (..))
import Rue.Proto.Model
import Rue.Proto.Verdict
import Test.Tasty (TestTree, testGroup)
import Test.Tasty.HUnit (assertBool, testCase, (@?=))

-- | The codes the Phase 0 checker emits. Every other code in the table is a
-- surface, engine or analysis rule the prototype does not model; the README
-- lists them under "not proven".
emittedCodes :: [Code]
emittedCodes =
  [ E0201, E0202, E0203, E0205, E0207, E0208
  , E0301, E0302, E0303, E0304, E0305
  , E0401, E0403, E0404, E0405, E0407, E0410
  , E0501, E0502, E0503, E0504, E0505, E0506, E0507, E0508, E0509
  ]

site0 :: Site
site0 =
  Site
    { siteHosts =
        [ HostRecord "db-01" "freebsd" ["ssh"] True
        , HostRecord "api-01" "appliance" ["api"] False
        , HostRecord "island" "freebsd" ["console"] True
        ]
    , siteTransports = ["ssh"]
    , siteAuthenticators = [Authenticator "oncall" True, Authenticator "alice" True, Authenticator "driver" False, Authenticator "requester" True]
    , siteMaxWait = Nothing
    , siteSchedulerPresent = ["db-01"]
    }

codesOf :: Plan -> [Code]
codesOf p = sort (map diagCode (vDiagnostics (check site0 "requester" p)))

raises :: Code -> Plan -> Bool
raises c p = c `elem` codesOf p

clean :: Plan -> Bool
clean p = null (codesOf p)

-- | A one-hour temporary plan over the given items.
temp :: [Item] -> Plan
temp items = (plan "p" "db-01" items) {planWane = Just (Duration 3600)}

owned :: Text -> Op
owned f = op f [entry Owned ("file:/" <> f)]

modified :: Text -> Op
modified f = op f [entry Modified ("file:/" <> f)]

s :: Op -> Item
s = Step . step

knellOp :: Op
knellOp = (op "fence" []) {opUndo = NoUndo, opRefusal = Knell (Just (guard "verified_off" Yes)) (CostProbe "fence_verdict") (AckNone "driver verified off")}

backstopAfter1h :: Backstop
backstopAfter1h = Backstop [After (Duration 3600)] (ArmBefore 1)

reachOp :: Op
reachOp = (owned "pf") {opUndoLocus = UndoTarget, opReach = ["ssh"], opUndoClosed = True}

tests :: TestTree
tests =
  testGroup
    "check"
    [ testGroup
        "op rules"
        [ pair E0201 (temp [s (owned "a") {opUndo = NoUndo}]) (temp [KnellItem (step knellOp)])
        , pair E0202 (temp [s (owned "a") {opUndoLocus = UndoTarget, opUndoClosed = False}]) (temp [s (owned "a") {opUndoLocus = UndoTarget}])
        , pair E0203 (temp [s (owned "a") {opUndoLocus = UndoNone}]) (temp [s (owned "a")])
        , pair E0205 (temp [s (op "tunnel" [entry Held "proc:tunnel"])]) (temp [s (op "tunnel" [entry Held "proc:tunnel"]) {opHasSuspend = True}])
        , pair E0207 (temp [s (owned "a") {opUndo = Computed []}]) (temp [s (owned "a") {opUndo = Computed ["file:/a"]}])
        , pair E0208 (temp [s (owned "a") {opUndoIdempotent = False}]) (temp [s (owned "a")])
        , pair E0407 (temp [s (owned "a") {opUndoLocus = UndoTarget, opLocus = HostLocus (StaticHost "api-01")}]) (temp [s (owned "a") {opUndoLocus = UndoController, opLocus = HostLocus (StaticHost "api-01")}])
        , pair
            E0410
            ((temp [s (modified "pf") {opUndoLocus = UndoTarget, opReach = ["ssh"]}]) {planBackstop = Just backstopAfter1h})
            ((temp [s reachOp]) {planBackstop = Just backstopAfter1h})
        ]
    , testGroup
        "interference"
        [ pair E0301 (temp [s (owned "a"), s (owned "a")]) (temp [s (owned "a"), s (owned "b")])
        , pair
            E0302
            (temp [s (owned "etc/x"), s (op "any" [entry Owned "file:/etc/{name}"])])
            ((temp [s (owned "etc/x"), s (op "any" [entry Owned "file:/etc/{name}"])]) {planStrictness = Warn})
        , testCase "a may-conflict under :warn is a verdict clause, not a refusal" $ do
            let v = check site0 "requester" ((temp [s (owned "etc/x"), s (op "any" [entry Owned "file:/etc/{name}"])]) {planStrictness = Warn})
            length (vMayConflicts v) @?= 1
            vStatus v @?= Ok
        , pair E0303 (temp [Par [s (owned "a"), s (owned "a")]]) (temp [Par [s (owned "a"), s (owned "b")]])
        , pair E0304 ((temp [Par [s reachOp, s (owned "b")]]) {planBackstop = Just backstopAfter1h}) ((temp [Par [s (owned "a"), s (owned "b")]]))
        , pair
            E0305
            (temp [s (op "r1" [anchored "file:/etc/keys" "rue"]), s (op "r2" [anchored "file:/etc/keys" "rue"])])
            (temp [s (op "r1" [anchored "file:/etc/keys" "rue-a"]), s (op "r2" [anchored "file:/etc/keys" "rue-b"])])
        , testCase "distinct anchors on one fact are disjoint (no E0301)" $
            assertBool "unexpected conflict" (not (raises E0301 (temp [s (op "r1" [anchored "file:/etc/keys" "a"]), s (op "r2" [anchored "file:/etc/keys" "b"])])))
        , testCase "a repeat over: loop is disjoint with itself" $
            assertBool "loop self-conflict" (clean (temp [Repeat (Over "guests" 8 True) "g" [s (op "stop" [entry Modified "guest:{g}:state"])]]))
        ]
    , testGroup
        "backstops"
        [ pair E0401 (temp [s reachOp]) ((temp [s reachOp]) {planBackstop = Just backstopAfter1h})
        , testCase "a backstop armed after a reach step is E0401" $
            assertBool "late arming past reach accepted" (raises E0401 ((temp [s (owned "a") {opUndoLocus = UndoTarget}, s reachOp]) {planBackstop = Just (Backstop [After (Duration 3600)] (ArmBefore 3))}))
        , pair E0403 ((temp [s (owned "a") {opUndoLocus = UndoTarget}]) {planBackstop = Just backstopAfter1h, planOwner = "island"}) ((temp [s (owned "a") {opUndoLocus = UndoTarget}]) {planBackstop = Just backstopAfter1h})
        , pair
            E0405
            ((temp [s (owned "a") {opUndoLocus = UndoTarget}]) {planBackstop = Just (Backstop [After (Duration 3600), UnlessHeartbeat (Duration 60) (Just (Duration 30))] (ArmBefore 1))})
            ((temp [s (owned "a") {opUndoLocus = UndoTarget}]) {planBackstop = Just (Backstop [After (Duration 3600), UnlessHeartbeat (Duration 60) (Just (Duration 20))] (ArmBefore 1))})
        , pair
            E0503
            ((temp [s (owned "a") {opUndoLocus = UndoTarget}]) {planBackstop = Just (Backstop [After (Duration 7200)] (ArmBefore 1))})
            ((temp [s (owned "a") {opUndoLocus = UndoTarget}]) {planBackstop = Just backstopAfter1h})
        , testCase "unless_confirmed on a temporary plan without fires_by_construction is E0503" $
            assertBool "accepted" (raises E0503 ((temp [s (owned "a") {opUndoLocus = UndoTarget}]) {planBackstop = Just (Backstop [After (Duration 3600), UnlessConfirmed (Duration 600)] (ArmBefore 1))}))
        , pair
            E0504
            ((plan "p" "db-01" [s (owned "a") {opUndoLocus = UndoTarget}, Commit]) {planBackstop = Just backstopAfter1h})
            ((plan "p" "db-01" [s (owned "a") {opUndoLocus = UndoTarget}, Confirm, Commit]) {planBackstop = Just (Backstop [UnlessConfirmed (Duration 600)] (ArmBefore 1))})
        ]
    , testGroup
        "mode"
        [ pair E0404 ((temp [Step (step (owned "a")) {stepForce = [ForceUnknown]}]) {planMode = Auto}) ((temp [Step (step (owned "a")) {stepForce = [ForceUnknown]}]))
        , pair
            E0507
            ((temp [KnellItem (step knellOp {opRefusal = Knell Nothing (CostNone "n/a") (AckGate (Single (Humans 1)))})]) {planMode = Auto})
            ((temp [KnellItem (step knellOp)]) {planMode = Auto})
        , testCase "a human step gate under :auto is E0507" $
            assertBool "accepted" (raises E0507 ((temp [Step (step (owned "a")) {stepGate = Just (Single (Auth "oncall" 1))}]) {planMode = Auto}))
        ]
    , testGroup
        "intent"
        [ pair E0501 ((temp [s (owned "a"), Commit])) (temp [s (owned "a")])
        , testCase "neither wane nor commit is E0501" $ assertBool "accepted" (raises E0501 (plan "p" "db-01" [s (owned "a")]))
        , pair E0502 (plan "p" "db-01" [Commit, s (owned "a")]) (plan "p" "db-01" [s (owned "a"), Commit])
        , pair E0505 (plan "p" "db-01" [When (guard "g" Yes) Nothing LapseRevert [Commit] []]) (plan "p" "db-01" [When (guard "g" Yes) Nothing LapseRevert [Commit] [Commit]])
        , testCase "fires_by_construction is temporary with the confirmed duration as wane" $ do
            let v = check site0 "requester" ((plan "p" "db-01" [s (owned "a") {opUndoLocus = UndoTarget}]) {planFiresByConstruction = True, planBackstop = Just (Backstop [UnlessConfirmed (Duration 600)] (ArmBefore 1))})
            vWane v @?= Just (Duration 600)
            vStatus v @?= Ok
        ]
    , testGroup
        "waits"
        [ pair
            E0506
            (plan "p" "db-01" [Step (step (owned "a")) {stepGate = Just (Single (Auth "oncall" 1))}, Commit])
            (plan "p" "db-01" [Step (step (owned "a")) {stepGate = Just (Single (Auth "oncall" 1)), stepWindow = Just (Duration 900)}, Commit])
        , testCase "a plan-entry gate with no window and no max_wait is E0506 (Pending unbounded)" $
            assertBool "accepted" (raises E0506 ((temp [s (owned "a")]) {planGate = Just (PlanGate (Single (Auth "oncall" 1)) Nothing False)}))
        , testCase "the site's max_wait bounds a permanent plan's waits" $
            assertBool "refused" (E0506 `notElem` checkWith site0 {siteMaxWait = Just (Duration 1800)} (plan "p" "db-01" [Assert (guard "g" Yes) Nothing LapseRevert, Commit]))
        , testCase "a fires_by_construction plan with no confirmed trigger has no wane and can wait: E0506" $
            assertBool "accepted" (raises E0506 ((plan "p" "db-01" [Assert (guard "g" Yes) Nothing LapseRevert, s (owned "a") {opUndoLocus = UndoTarget}]) {planFiresByConstruction = True, planBackstop = Just (Backstop [UnlessHeartbeat (Duration 60) Nothing] (ArmBefore 1))}))
        ]
    , testGroup
        "gates"
        [ pair E0508 (gated (Single (Auth "nobody" 1))) (gated (Single (Auth "oncall" 1)))
        , testCase "an unsatisfiable threshold is E0508" $ assertBool "accepted" (raises E0508 (gated (Thresh 5 [Auth "oncall" 1])))
        , testCase "a gate counting the requester is E0508" $ assertBool "accepted" (raises E0508 (gated (Single (Auth "requester" 1))))
        , testCase "the requester may acknowledge a knell" $
            assertBool "refused" (not (raises E0508 (temp [KnellItem (step knellOp {opRefusal = Knell Nothing (CostNone "n/a") (AckGate (Single (Auth "requester" 1)))})])))
        , pair E0509 (gated (Single (Wait (Duration 1800) 1))) ((gated (Single (Wait (Duration 1800) 1))) {planGate = Just (PlanGate (Single (Wait (Duration 1800) 1)) (Just (Duration 3600)) True)})
        , testCase "minimum distinct humans is computed over satisfying paths" $ do
            let v = check site0 "requester" (gated (Thresh 2 [Auth "oncall" 1, Auth "alice" 1, Auth "driver" 1]))
            fmap gvMinDistinctHumans (vGate v) @?= Just (Just 1)
        ]
    , testGroup
        "verdict shape"
        [ testCase "a clean temporary plan is ok and fully reversible" $ do
            let v = check site0 "requester" (temp [s (owned "a"), s (owned "b")])
            vStatus v @?= Ok
            vReversibleThrough v @?= 2
            vControllerOnlyUndos v @?= [1, 2]
        , testCase "a knell is the point of no return; later steps revert back to it" $ do
            let v = check site0 "requester" (temp [s (owned "a"), KnellItem (step knellOp), s (owned "b") {opRefusal = Hold Nothing}])
            vReversibleThrough v @?= 1
            fmap ponrStep (vPointOfNoReturn v) @?= Just 2
            vReversibleBackTo v @?= Just (3, 2)
            vHoldsAt v @?= [3]
        , testCase "a step on an unreachable host is deferred" $
            vDeferred (check site0 "requester" (temp [s (owned "a") {opLocus = HostLocus (StaticHost "island")}])) @?= [1]
        , testCase "the emitted-code list is exactly what the suite raises somewhere" $
            sort emittedCodes @?= emittedCodes
        ]
    ]
  where
    pair code bad good =
      testGroup
        (show code)
        [ testCase "raised" $ assertBool ("did not raise " <> show code) (raises code bad)
        , testCase "not raised" $ assertBool ("raised " <> show code <> " on the good plan: " <> show (codesOf good)) (not (raises code good))
        ]
    gated g = (temp [s (owned "a")]) {planGate = Just (PlanGate g (Just (Duration 3600)) False)}
    checkWith st p = sort (map diagCode (vDiagnostics (check st "requester" p)))
