{-# LANGUAGE OverloadedStrings #-}
-- | What a tenant is to the prototype: a site, a requester, and the plans
-- checked per host, plus the artifacts each produces.
module Rue.Proto.Tenants.Common
  ( Tenant (..)
  , Case (..)
  , NegativeCase (..)
  , tenantArtifacts
  , negativeArtifacts
  , verdictOf
  ) where

import Data.Text (Text)
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import Rue.Proto.Check (check, deferredSteps)
import Rue.Proto.Diagnostics (Code, codeText)
import Rue.Proto.Explain (explain)
import Rue.Proto.Golden (Artifact (..))
import qualified Rue.Proto.Json.Canonical as Canonical
import qualified Rue.Proto.Json.PlanIr as PlanIr
import Rue.Proto.Model (Plan, Site)
import Rue.Proto.Prose (prose)
import Rue.Proto.Verdict (Verdict, toJson)

-- | One plan checked on one host.
data Case = Case
  { caseHost :: Text -- ^ directory name under expected/
  , casePlan :: Plan
  }

data Tenant = Tenant
  { tenantName :: Text -- ^ directory name under tenants/
  , tenantSite :: Site
  , tenantRequester :: Text
  , tenantCases :: [Case]
  }

-- | A plan the checker must refuse with a named code.
data NegativeCase = NegativeCase
  { negCode :: Code
  , negSlug :: Text
  , negSite :: Site
  , negRequester :: Text
  , negPlan :: Plan
  }

verdictOf :: Site -> Text -> Plan -> Verdict
verdictOf = check

tenantArtifacts :: Tenant -> [Artifact]
tenantArtifacts t = concatMap one (tenantCases t)
  where
    one c =
      let v = check (tenantSite t) (tenantRequester t) (casePlan c)
          dir = T.unpack ("tenants/" <> tenantName t <> "/expected/" <> caseHost c <> "/")
       in [ Artifact (dir <> "plan.json") (Canonical.encode (PlanIr.toJson (tenantSite t) (tenantRequester t) (casePlan c)))
          , Artifact (dir <> "verdict.json") (Canonical.encode (toJson v))
          , Artifact (dir <> "verdict.txt") (Right (TE.encodeUtf8 (prose v)))
          , Artifact (dir <> "explain.txt") (Right (TE.encodeUtf8 (explain (casePlan c) (deferredSteps (tenantSite t) (casePlan c)))))
          ]

negativeArtifacts :: NegativeCase -> [Artifact]
negativeArtifacts n =
  let v = check (negSite n) (negRequester n) (negPlan n)
      dir = T.unpack ("tenants/_negative/" <> codeText (negCode n) <> "-" <> negSlug n <> "/expected/")
   in [ Artifact (dir <> "plan.json") (Canonical.encode (PlanIr.toJson (negSite n) (negRequester n) (negPlan n)))
      , Artifact (dir <> "verdict.json") (Canonical.encode (toJson v))
      , Artifact (dir <> "verdict.txt") (Right (TE.encodeUtf8 (prose v)))
      ]
