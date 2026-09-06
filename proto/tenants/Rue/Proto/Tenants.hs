-- | The acceptance tenants and everything golden-tested about them.
--
-- The list is the authority on which goldens exist: the test-suite compares
-- exactly these, and any expected file no artifact claims is an orphan and a
-- failure.
module Rue.Proto.Tenants
  ( artifacts
  , tenants
  , negatives
  , lookupCase
  ) where

import Data.Text (Text)
import qualified Data.Text.Encoding as TE
import Rue.Proto.Golden (Artifact (..))
import Rue.Proto.Model (Plan, Site)
import Rue.Proto.States (renderTable)
import Rue.Proto.Tenants.Common
import qualified Rue.Proto.Tenants.Negative as Negative
import qualified Rue.Proto.Tenants.T1 as T1
import qualified Rue.Proto.Tenants.T2 as T2
import qualified Rue.Proto.Tenants.T3 as T3
import qualified Rue.Proto.Tenants.T4 as T4

tenants :: [Tenant]
tenants = [T1.tenant, T2.tenant, T3.tenant, T4.tenant]

negatives :: [NegativeCase]
negatives = Negative.cases

-- | Every golden artifact, with its path relative to the repository root.
artifacts :: [Artifact]
artifacts =
  concatMap tenantArtifacts tenants
    <> concatMap negativeArtifacts negatives
    <> [Artifact "docs/state-transitions.tsv" (Right (TE.encodeUtf8 renderTable))]

-- | A tenant's case by tenant name and host directory, for @rue-proto-check@.
lookupCase :: Text -> Text -> Maybe (Site, Text, Plan)
lookupCase name host =
  case [(tenantSite t, tenantRequester t, casePlan c) | t <- tenants, tenantName t == name, c <- tenantCases t, caseHost c == host] of
    x : _ -> Just x
    [] -> Nothing
