{-# LANGUAGE OverloadedStrings #-}
-- | The core model, docs/ROADMAP.md sections 5.1 to 5.4, as Haskell terms.
--
-- Names are the roadmap's. Where the prototype simplifies, the simplification
-- is named: bodies are opaque here (closure of a @:target@ undo is a boolean
-- the tenant asserts, per Phase 0's "not proven" table), facts are named by
-- shape strings, and guards carry a fixed 'Tri' so a plan's verdict can be
-- computed without a world.
module Rue.Proto.Model
  ( -- * Scalars
    Host
  , Duration (..)
  , renderDuration
  , Tri (..)
    -- * Facts and footprints
  , Kind (..)
  , FootprintEntry (..)
  , Footprint
  , entry
  , anchored
    -- * Guards
  , Guard (..)
  , guard
    -- * Ops
  , Locus (..)
  , HostRef (..)
  , UndoLocus (..)
  , Undo (..)
  , Cost (..)
  , Ack (..)
  , Refusal (..)
  , Drift (..)
  , defaultDrift
  , Output (..)
  , Op (..)
  , op
    -- * Gates
  , Authenticator (..)
  , GateExpr (..)
  , Factor (..)
  , PlanGate (..)
    -- * Plans and items
  , Direction (..)
  , ForceName (..)
  , OnLapse (..)
  , StepI (..)
  , step
  , RepeatForm (..)
  , Item (..)
  , Trigger (..)
  , ArmBefore (..)
  , Backstop (..)
  , Strictness (..)
  , Mode (..)
  , JournalRequirement (..)
  , Plan (..)
  , plan
    -- * The site and the world the checker is handed
  , HostRecord (..)
  , Site (..)
  ) where

import Data.Text (Text)
import qualified Data.Text as T

-- ---------------------------------------------------------------------------
-- Scalars

type Host = Text

-- | Whole seconds. Enough for every tenant; a finer unit is a Phase 1 choice.
newtype Duration = Duration {seconds :: Int}
  deriving (Eq, Ord, Show)

-- | The surface spelling: the largest unit that divides evenly, else seconds.
renderDuration :: Duration -> Text
renderDuration (Duration s)
  | s /= 0 && s `mod` 86400 == 0 = T.pack (show (s `div` 86400)) <> "d"
  | s /= 0 && s `mod` 3600 == 0 = T.pack (show (s `div` 3600)) <> "h"
  | s /= 0 && s `mod` 60 == 0 = T.pack (show (s `div` 60)) <> "m"
  | otherwise = T.pack (show s) <> "s"

data Tri = Yes | No | Unknown
  deriving (Eq, Ord, Show, Enum, Bounded)

-- ---------------------------------------------------------------------------
-- Facts and footprints (5.2)

data Kind = Owned | Region | Modified | Derived | AppendOnly | Held
  deriving (Eq, Ord, Show, Enum, Bounded)

-- | A footprint entry. The shape is static and names the fact
-- (e.g. @file:/etc/pf.conf@ on the step's host); the instance, when bound,
-- is what a value flowing in made concrete; the anchor is a region's fence.
data FootprintEntry = FootprintEntry
  { fpKind :: Kind
  , fpShape :: Text
  , fpInstance :: Maybe Text
  , fpAnchor :: Maybe Text
  }
  deriving (Eq, Ord, Show)

type Footprint = [FootprintEntry]

entry :: Kind -> Text -> FootprintEntry
entry k s = FootprintEntry k s Nothing Nothing

anchored :: Text -> Text -> FootprintEntry
anchored s a = FootprintEntry Region s Nothing (Just a)

-- ---------------------------------------------------------------------------
-- Guards (5.1)

-- | A guard is an expression yielding a 'Tri'. The prototype has no
-- evaluator, so a guard carries the value a check should assume, its name
-- (what @force:@ refers to), and whether it declares @force: never@.
data Guard = Guard
  { guardName :: Text
  , guardValue :: Tri
  , guardForceNever :: Bool
  }
  deriving (Eq, Ord, Show)

guard :: Text -> Tri -> Guard
guard n v = Guard n v False

-- ---------------------------------------------------------------------------
-- Ops (5.3)

data HostRef
  = StaticHost Host
  | BoundHost Text -- ^ bound at runtime from a named output
  deriving (Eq, Ord, Show)

data Locus = Controller | Target | HostLocus HostRef
  deriving (Eq, Ord, Show)

data UndoLocus = UndoTarget | UndoController | UndoNone
  deriving (Eq, Ord, Show)

-- | The undo. @Computed@ and @Compensate@ carry their declared @undo_pre@
-- as the shapes the undo needs unchanged; @Restore@ derives it from the
-- footprint (each modified fact equals its post-do value).
data Undo
  = Restore
  | Computed [Text] -- ^ the declared @undo_pre@ shapes
  | Compensate [Text] -- ^ the declared @undo_pre@ shapes
  | NoUndo
  deriving (Eq, Ord, Show)

data Cost = CostProbe Text | CostNone Text
  deriving (Eq, Ord, Show)

data Ack = AckGate GateExpr | AckNone Text
  deriving (Eq, Ord, Show)

data Refusal
  = Revert
  | Hold (Maybe Text) -- ^ @hold_via:@ op, if any
  | Knell (Maybe Guard) Cost Ack -- ^ guard, cost, acknowledgement
  deriving (Eq, Ord, Show)

data Drift = Clobber | Defer
  deriving (Eq, Ord, Show)

-- | The default drift policy by footprint kind (section 5.2).
defaultDrift :: Kind -> Maybe Drift
defaultDrift k = case k of
  Owned -> Just Clobber
  Region -> Just Clobber
  Modified -> Just Defer
  Derived -> Nothing
  AppendOnly -> Nothing
  Held -> Nothing

data Output = Output {outputName :: Text, outputSecret :: Bool}
  deriving (Eq, Ord, Show)

data Op = Op
  { opId :: Text
  , opFootprint :: Footprint
  , opPre :: [Guard]
  , opUndo :: Undo
  , opPost :: [Guard]
  , opUndoLocus :: UndoLocus
  , opRefusal :: Refusal
  , opDrift :: Maybe Drift -- ^ 'Nothing' means the kind's default
  , opReach :: [Text] -- ^ transports this op may sever
  , opOutputs :: [Output]
  , opExclusivity :: Maybe Text
  , opLocus :: Locus
  , opHasSuspend :: Bool -- ^ @suspend:@ and @reestablish:@ defined
  , opHandoffDone :: Maybe Text -- ^ probe that continues a deferred step
  , opUndoClosed :: Bool -- ^ Phase 0 stands in for the closure analysis
  , opUndoIdempotent :: Bool -- ^ Phase 0 stands in for the idempotency analysis
  , opUndoOneLine :: Text -- ^ the undo line @explain@ prints
  }
  deriving (Eq, Ord, Show)

-- | An op with every optional field at its quiet default.
op :: Text -> Footprint -> Op
op i fp =
  Op
    { opId = i
    , opFootprint = fp
    , opPre = []
    , opUndo = Restore
    , opPost = []
    , opUndoLocus = UndoController
    , opRefusal = Revert
    , opDrift = Nothing
    , opReach = []
    , opOutputs = []
    , opExclusivity = Nothing
    , opLocus = Target
    , opHasSuspend = False
    , opHandoffDone = Nothing
    , opUndoClosed = True
    , opUndoIdempotent = True
    , opUndoOneLine = "restore"
    }

-- ---------------------------------------------------------------------------
-- Gates (5.11)

data Authenticator = Authenticator {authId :: Text, authHuman :: Bool}
  deriving (Eq, Ord, Show)

data GateExpr
  = Thresh Int [Factor]
  | Single Factor
  deriving (Eq, Ord, Show)

data Factor
  = Auth Text Int -- ^ authenticator id, weight
  | Humans Int -- ^ any human authenticator, weight
  | Group GateExpr Int
  | Wait Duration Int
  deriving (Eq, Ord, Show)

data PlanGate = PlanGate
  { gateExpr :: GateExpr
  , gateWindow :: Maybe Duration
  , allowZeroHuman :: Bool
  }
  deriving (Eq, Ord, Show)

-- ---------------------------------------------------------------------------
-- Plans and items (5.4)

-- | Which way a step runs. 'reverse' flips it; that is all reversal is,
-- syntactically, and it is what makes @reverse . reverse = id@ a law.
data Direction = Forward | Inverse
  deriving (Eq, Ord, Show)

data ForceName = ForceGuard Text | ForceDrift | ForceUnknown
  deriving (Eq, Ord, Show)

data OnLapse = LapseRevert | LapseHold
  deriving (Eq, Ord, Show)

data StepI = StepI
  { stepOp :: Op
  , stepDirection :: Direction
  , stepGate :: Maybe GateExpr
  , stepWindow :: Maybe Duration
  , stepOnLapse :: OnLapse
  , stepForce :: [ForceName]
  , stepAlias :: Maybe Text
  , stepArgs :: [Text] -- ^ rendered arguments, for @explain@
  }
  deriving (Eq, Ord, Show)

step :: Op -> StepI
step o =
  StepI
    { stepOp = o
    , stepDirection = Forward
    , stepGate = Nothing
    , stepWindow = Nothing
    , stepOnLapse = LapseRevert
    , stepForce = []
    , stepAlias = Nothing
    , stepArgs = []
    }

data RepeatForm
  = Count Int
  | Over Text Int Bool -- ^ list expression, literal cap, set-valued?
  deriving (Eq, Ord, Show)

data Item
  = Step StepI
  | Par [Item]
  | Slot Text
  | KnellItem StepI -- ^ the op must have @refusal: Knell@ (E0201)
  | Confirm
  | Commit
  | Preflight [Guard]
  | Observe Text Text -- ^ probe, alias
  | Assert Guard (Maybe Duration) OnLapse -- ^ guard, window, on_lapse
  | Repeat RepeatForm Text [Item] -- ^ form, variable, body
  | When Guard (Maybe Duration) OnLapse [Item] [Item] -- ^ guard, window, on_lapse, then, else
  deriving (Eq, Ord, Show)

data Trigger
  = After Duration
  | UnlessConfirmed Duration
  | UnlessHeartbeat Duration (Maybe Duration) -- ^ deadline, interval
  deriving (Eq, Ord, Show)

-- | Where arming happens relative to the numbered steps: before step @n@.
-- @ArmBefore (last + 1)@ is late arming after the last covered step.
newtype ArmBefore = ArmBefore Int
  deriving (Eq, Ord, Show)

data Backstop = Backstop
  { triggers :: [Trigger]
  , armBefore :: ArmBefore
  }
  deriving (Eq, Ord, Show)

data Strictness = Strict | Warn
  deriving (Eq, Ord, Show)

data Mode = Manual | Auto
  deriving (Eq, Ord, Show)

data JournalRequirement = Chained | Signed
  deriving (Eq, Ord, Show)

data Plan = Plan
  { planId :: Text
  , planOwner :: Host
  , planGate :: Maybe PlanGate
  , planWane :: Maybe Duration
  , planRenewWithin :: Maybe Duration
  , planBackstop :: Maybe Backstop
  , planFiresByConstruction :: Bool
  , planStrictness :: Strictness
  , planMode :: Mode
  , planExclusivity :: Maybe Text
  , planRequireJournal :: Maybe JournalRequirement
  , planBody :: [Item]
  }
  deriving (Eq, Ord, Show)

plan :: Text -> Host -> [Item] -> Plan
plan i h body =
  Plan
    { planId = i
    , planOwner = h
    , planGate = Nothing
    , planWane = Nothing
    , planRenewWithin = Nothing
    , planBackstop = Nothing
    , planFiresByConstruction = False
    , planStrictness = Strict
    , planMode = Manual
    , planExclusivity = Nothing
    , planRequireJournal = Nothing
    , planBody = body
    }

-- ---------------------------------------------------------------------------
-- The world the checker is handed

-- | The parts of a host record a check reads: whether an executor can reach
-- it at all, and whether that executor has a filesystem (an instance
-- directory can live there; a @:target@ undo is possible).
data HostRecord = HostRecord
  { hrName :: Host
  , hrOs :: Text
  , hrReach :: [Text]
  , hrFilesystem :: Bool
  }
  deriving (Eq, Ord, Show)

data Site = Site
  { siteHosts :: [HostRecord]
  , siteTransports :: [Text] -- ^ transports the declared executors serve
  , siteAuthenticators :: [Authenticator]
  , siteMaxWait :: Maybe Duration
  , siteSchedulerPresent :: [Host] -- ^ hosts whose scheduler binding reports presence
  }
  deriving (Eq, Ord, Show)
