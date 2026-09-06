-- | The acceptance tenants and everything golden-tested about them.
--
-- The list is the authority on which goldens exist: the test-suite compares
-- exactly these, and any expected file no artifact claims is an orphan and a
-- failure. This module fills in as the tenants are encoded.
module Rue.Proto.Tenants
  ( artifacts
  ) where

import qualified Data.Text.Encoding as TE
import Rue.Proto.Golden (Artifact (..))
import Rue.Proto.States (renderTable)

-- | Every golden artifact, with its path relative to the repository root.
artifacts :: [Artifact]
artifacts =
  [ Artifact "docs/state-transitions.tsv" (Right (TE.encodeUtf8 renderTable))
  ]
