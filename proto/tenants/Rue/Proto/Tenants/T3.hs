{-# LANGUAGE OverloadedStrings #-}
-- | T3: commit-confirmed firewall change (docs/ROADMAP.md section 8.3).
-- Permanent.
--
-- A region change to a host firewall with reach ssh(host); undo locus
-- :target; a backstop [unless_confirmed: 10m] installed and armed before the
-- change; a reachability probe; confirm(); commit() last. A Windows variant
-- with a firewall rule the tenant declares.
module Rue.Proto.Tenants.T3 (tenant, site, openMgmtPort, openMgmtPortWindows, pfAllow) where

import Rue.Proto.Model
import Rue.Proto.Tenants.Common

site :: Site
site =
  Site
    { siteHosts =
        [ HostRecord "fw-01" "freebsd" ["ssh"] True
        , HostRecord "fw-win-01" "windows" ["ssh"] True
        ]
    , siteTransports = ["ssh"]
    , siteAuthenticators = [Authenticator "netops" True]
    , siteMaxWait = Nothing
    , siteSchedulerPresent = ["fw-01", "fw-win-01"]
    }

pfAllow :: Op
pfAllow =
  (op "pf_allow" [anchored "file:/etc/pf.conf" "rue-mgmt"])
    { opUndoLocus = UndoTarget
    , opReach = ["ssh"]
    , opUndoOneLine = "strip the rue-mgmt anchor from /etc/pf.conf; pfctl reload"
    }

winfwAllow :: Op
winfwAllow =
  (op "winfw_allow" [entry Owned "winfw:rule:rue-mgmt"])
    { opUndoLocus = UndoTarget
    , opReach = ["ssh"]
    , opUndoOneLine = "remove the rue-mgmt firewall rule"
    }

confirmed :: Text' -> Op -> Plan
confirmed host change =
  (plan "open_mgmt_port" host [Step (step change) {stepArgs = ["port: 8443"]}, Observe "verify_reach" "reach", Confirm, Commit])
    { planBackstop = Just (Backstop [UnlessConfirmed (Duration 600)] (ArmBefore 1))
    }

type Text' = Host

openMgmtPort :: Plan
openMgmtPort = confirmed "fw-01" pfAllow

openMgmtPortWindows :: Plan
openMgmtPortWindows = confirmed "fw-win-01" winfwAllow

tenant :: Tenant
tenant =
  Tenant
    { tenantName = "t3"
    , tenantSite = site
    , tenantRequester = "netops_requester"
    , tenantCases = [Case "fw-01" openMgmtPort, Case "fw-win-01" openMgmtPortWindows]
    }
