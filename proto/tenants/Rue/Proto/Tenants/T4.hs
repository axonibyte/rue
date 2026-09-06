{-# LANGUAGE OverloadedStrings #-}
-- | T4: a temporary override in a reactive host (docs/ROADMAP.md section
-- 8.4). Temporary, embedded: journal, inventory and execution through hooks.
--
-- An op shed_load whose footprint is a group of actuator facts (modified,
-- equivalence = reported state) with a restorative undo, a controller undo
-- locus (no instance directory; "reverts only while the engine lives") and
-- drift: :clobber, the host's model being convergent; a wane; the plan the
-- host fires from a state-machine clause in mode :manual and recants on
-- exit. A second variant with drift: :defer.
module Rue.Proto.Tenants.T4 (tenant, site, shedLoad, shedLoadDeferring) where

import Rue.Proto.Model
import Rue.Proto.Tenants.Common

site :: Site
site =
  Site
    { siteHosts = [HostRecord "site-ctl" "reactive-host" ["actuate"] False]
    , siteTransports = ["actuate"]
    , siteAuthenticators = [Authenticator "site_operator" True]
    , siteMaxWait = Nothing
    , siteSchedulerPresent = []
    }

shedLoadOp :: Drift -> Op
shedLoadOp d =
  (op "shed_load" [entry Modified "actuator:hvac-1:state", entry Modified "actuator:hvac-2:state", entry Modified "actuator:pump-1:state"])
    { opUndoLocus = UndoController
    , opDrift = Just d
    , opUndoOneLine = "restore the three actuators to their reported pre-shed state"
    }

shedLoad :: Plan
shedLoad = (plan "shed_load" "site-ctl" [Step (step (shedLoadOp Clobber))]) {planWane = Just (Duration 7200)}

shedLoadDeferring :: Plan
shedLoadDeferring = (plan "shed_load_deferring" "site-ctl" [Step (step (shedLoadOp Defer))]) {planWane = Just (Duration 7200)}

tenant :: Tenant
tenant =
  Tenant
    { tenantName = "t4"
    , tenantSite = site
    , tenantRequester = "reactive_host"
    , tenantCases = [Case "site-ctl" shedLoad, Case "site-ctl-defer" shedLoadDeferring]
    }
