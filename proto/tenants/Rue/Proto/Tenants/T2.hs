{-# LANGUAGE OverloadedStrings #-}
-- | T2: cluster succession (docs/ROADMAP.md section 8.2). Permanent, ending
-- in commit().
--
-- The promote ladder: rungs as three-valued guards; a probes rung with
-- force: never; a fence rung as a knell whose guard is the driver's
-- verified-off, with a cost probe and ack: :none under auto or one human
-- under manual; per-guest steps after the knell with refusal :hold; a second
-- knell for destructive rollback of ahead datasets, reachable only on the
-- manual path; succession log and placement as append_only; a resurrection
-- gate's hold via a platform op; a per-guest heir on another node as
-- deferred; exclusivity per corpse; failback's written-bytes guard measured
-- twice; a repeat over: per-guest loop that checks clean under strict.
module Rue.Proto.Tenants.T2 (tenant, site, promoteAuto, promoteManual) where

import Rue.Proto.Model
import Rue.Proto.Tenants.Common

site :: Site
site =
  Site
    { siteHosts =
        [ HostRecord "node-b" "freebsd" ["ssh"] True
        , HostRecord "node-a" "freebsd" ["ssh"] True
        , HostRecord "node-c" "freebsd" ["console"] True
        ]
    , siteTransports = ["ssh"]
    , siteAuthenticators = [Authenticator "operator" True, Authenticator "second_operator" True, Authenticator "fence_driver" False]
    , siteMaxWait = Just (Duration 1800)
    , siteSchedulerPresent = ["node-b"]
    }

fence :: Ack -> Op
fence ack =
  (op "fence_corpse" [])
    { opUndo = NoUndo
    , opUndoLocus = UndoNone
    , opRefusal = Knell (Just (guard "fence_verified_off" Yes)) (CostProbe "fence_verdict") ack
    , opLocus = Controller
    , opUndoOneLine = ""
    }

resurrectionGate :: Op
resurrectionGate =
  (op "resurrection_gate" [entry Modified "platform:node-a:mode"])
    { opRefusal = Hold (Just "slave_mode")
    , opUndoLocus = UndoController
    , opLocus = Controller
    , opUndoOneLine = "release the slave-mode gate on node-a"
    }

startGuest :: Op
startGuest =
  (op "start_guest" [entry Modified "guest:{g}:state"])
    { opRefusal = Hold Nothing
    , opUndoLocus = UndoController
    , opUndoOneLine = "stop guest {g} on node-b"
    }

zfsRollback :: Op
zfsRollback =
  (op "rollback_ahead_datasets" [])
    { opUndo = NoUndo
    , opUndoLocus = UndoNone
    , opRefusal = Knell (Just (guard "datasets_ahead" Yes)) (CostProbe "destroyed_snapshots") (AckGate (Single (Humans 1)))
    , opUndoOneLine = ""
    }

successionLog :: Op
successionLog =
  (op "record_succession" [entry AppendOnly "file:/var/db/succession.log", entry AppendOnly "record:placement"])
    { opUndo = Compensate ["file:/var/db/succession.log", "record:placement"]
    , opUndoLocus = UndoController
    , opLocus = Controller
    , opUndoOneLine = "append a reversal record (undone by record, not erasure)"
    }

heirOnOtherNode :: Op
heirOnOtherNode =
  (op "start_heir" [entry Modified "guest:heir:state"])
    { opRefusal = Hold Nothing
    , opUndoLocus = UndoController
    , opLocus = HostLocus (StaticHost "node-c")
    , opHandoffDone = Just "heir_running_on_c"
    , opUndoOneLine = "stop the heir on node-c"
    }

ladder :: Ack -> [Item] -> Plan
ladder ack manualOnly =
  (plan "promote" "node-b" body)
    { planExclusivity = Just "corpse:node-a"
    }
  where
    body =
      [ Preflight [guard "written_bytes_since_split" Yes]
      , Assert (guard "peer_dead" Yes) Nothing LapseRevert
      , Assert (Guard "probes_agree" Yes True) Nothing LapseRevert
      , KnellItem (step (fence ack))
      ]
        <> manualOnly
        <> [ Step (step resurrectionGate)
           , Repeat (Over "guests" 16 True) "g" [Step (step startGuest)]
           , Step (step successionLog)
           , Step (step heirOnOtherNode)
           , Commit
           ]

promoteAuto :: Plan
promoteAuto = (ladder (AckNone "the driver's verified-off is the automation's own evidence") []) {planMode = Auto, planId = "promote_auto"}

promoteManual :: Plan
promoteManual =
  ladder
    (AckGate (Single (Humans 1)))
    [When (guard "datasets_ahead" Yes) Nothing LapseRevert [KnellItem (step zfsRollback)] []]

tenant :: Tenant
tenant =
  Tenant
    { tenantName = "t2"
    , tenantSite = site
    , tenantRequester = "operator"
    , tenantCases = [Case "node-b-auto" promoteAuto, Case "node-b-manual" promoteManual]
    }
