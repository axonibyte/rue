{-# LANGUAGE OverloadedStrings #-}
-- | T1: break-glass access (docs/ROADMAP.md section 8.1). Temporary.
--
-- Four channel ops: a service-posture drop-in as owned with a derived verify;
-- a fenced block in a shared authorized-keys file as region; a
-- management-controller account enable as modified with a secret output and
-- a controller undo, on an API host with no instance directory; a console
-- tunnel as held with suspend/reestablish and a rotated secret. A plan-entry
-- gate through the approval hook; wane 4h with renewal; a :target backstop
-- [after: 4h, unless_heartbeat: 60s] covering the ssh-borne ops, armed after
-- them (reach empty, late arming); scheduler presence as a precondition.
module Rue.Proto.Tenants.T1 (tenant, site, breakglass) where

import Rue.Proto.Model
import Rue.Proto.Tenants.Common

site :: Site
site =
  Site
    { siteHosts =
        [ HostRecord "db-01" "freebsd" ["ssh"] True
        , HostRecord "bmc-01" "appliance" ["api"] False
        ]
    , siteTransports = ["ssh", "api"]
    , siteAuthenticators = [Authenticator "oncall" True, Authenticator "platform_a" True]
    , siteMaxWait = Nothing
    , siteSchedulerPresent = ["db-01"]
    }

sshdPosture :: Op
sshdPosture =
  (op "service_posture" [entry Owned "file:/etc/ssh/sshd_config.d/rue-breakglass.conf", entry Derived "probe:sshd_posture"])
    { opUndoLocus = UndoTarget
    , opUndoOneLine = "remove /etc/ssh/sshd_config.d/rue-breakglass.conf; reload sshd"
    , opPost = [guard "sshd_posture_applied" Yes]
    }

authorizedKeysBlock :: Op
authorizedKeysBlock =
  (op "authorized_keys_block" [anchored "file:/root/.ssh/authorized_keys" "rue-breakglass"])
    { opUndoLocus = UndoTarget
    , opUndoOneLine = "strip the rue-breakglass block from /root/.ssh/authorized_keys"
    }

bmcAccount :: Op
bmcAccount =
  (op "bmc_account_enable" [entry Modified "bmc:account:breakglass"])
    { opUndoLocus = UndoController
    , opLocus = HostLocus (StaticHost "bmc-01")
    , opOutputs = [Output "bmc_password" True]
    , opUndoOneLine = "disable the breakglass account and clear bmc_password"
    }

vncConsole :: Op
vncConsole =
  (op "console_tunnel" [entry Held "proc:vnc-tunnel"])
    { opUndoLocus = UndoController
    , opLocus = Controller
    , opHasSuspend = True
    , opOutputs = [Output "console_secret" True]
    , opUndoOneLine = "release the tunnel and rotate console_secret"
    }

breakglass :: Plan
breakglass =
  (plan "breakglass" "db-01" [Step (step sshdPosture), Step (step authorizedKeysBlock), Step (step bmcAccount) {stepAlias = Just "bmc"}, Step (step vncConsole)])
    { planGate = Just (PlanGate (Single (Auth "oncall" 1)) (Just (Duration 1800)) False)
    , planWane = Just (Duration 14400)
    , planRenewWithin = Just (Duration 1800)
    , planBackstop = Just (Backstop [After (Duration 14400), UnlessHeartbeat (Duration 60) (Just (Duration 20))] (ArmBefore 3))
    , planExclusivity = Just "breakglass"
    }

tenant :: Tenant
tenant =
  Tenant
    { tenantName = "t1"
    , tenantSite = site
    , tenantRequester = "ops_requester"
    , tenantCases = [Case "db-01" breakglass]
    }
