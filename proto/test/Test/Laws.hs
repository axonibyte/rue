{-# LANGUAGE OverloadedStrings #-}
-- | The laws of section 5.5, as properties over generated plans.
module Test.Laws (tests, genKnellFree, genItem) where

import qualified Data.Text as T
import Rue.Proto.Algebra
import Rue.Proto.Model
import Test.Tasty (TestTree, testGroup)
import Test.Tasty.HUnit (testCase, (@?=))
import Test.Tasty.QuickCheck

tests :: TestTree
tests =
  testGroup
    "laws"
    [ testProperty "reverse-reverse: reverse . reverse = id on knell-free plans" $
        forAll genKnellFree $ \items ->
          (reverseItems items >>= reverseItems) === Right items
    , testProperty "reverse-seq: reverse (seq a b) = seq (reverse b) (reverse a)" $
        forAll ((,) <$> genKnellFree <*> genKnellFree) $ \(a, b) ->
          reverseItems (seq_ a b) === ((\rb ra -> seq_ rb ra) <$> reverseItems b <*> reverseItems a)
    , testProperty "reverse-par: reverse (par xs) = par (map reverse xs)" $
        forAll genKnellFree $ \xs ->
          reverseItem (par_ xs) === (Par <$> traverse reverseItem xs)
    , testProperty "reverse-knell: a plan with a knell anywhere refuses to reverse" $
        forAll genWithKnell $ \items ->
          case reverseItems items of
            Left (Refused (KnellItem _)) -> property True
            other -> counterexample ("did not refuse: " <> show other) False
    , testProperty "reverse-from: undoing the first k leaves is the reverse of those leaves" $
        forAll genKnellFree $ \items -> forAll (choose (0, length (leaves items))) $ \k ->
          reverseFrom k items === traverse reverseItem (reverse (take k (leaves items)))
    , testProperty "reversing a step flips its direction and nothing else" $
        forAll genStep $ \s ->
          reverseItem (Step s) === Right (Step s {stepDirection = flipD (stepDirection s)})
    , testCase "numbering counts leaves, not containers" $
        map fst (numbered [Step s1, Par [Step s1, Step s1], Confirm, Commit]) @?= [1, 2, 3, 4, 5]
    ]
  where
    flipD Forward = Inverse
    flipD Inverse = Forward
    s1 = step (op "a" [entry Owned "file:/a"])

-- | A knell-free plan of modest depth.
genKnellFree :: Gen [Item]
genKnellFree = sized $ \n -> few n (genItem n)

genWithKnell :: Gen [Item]
genWithKnell = do
  pre <- genKnellFree
  k <- genStep
  post <- genKnellFree
  pure (pre <> [KnellItem k] <> post)

genItem :: Int -> Gen Item
genItem n
  | n <= 0 = Step <$> genStep
  | otherwise =
      frequency
        [ (6, Step <$> genStep)
        , (1, Par <$> few (n `div` 3) (genItem (n `div` 3)))
        , (1, pure Confirm)
        , (1, Observe <$> genName <*> genName)
        , (1, (\g -> Assert g Nothing LapseRevert) <$> genGuard)
        , (1, (\body -> Repeat (Count 2) "i" body) <$> few (n `div` 3) (genItem (n `div` 3)))
        , (1, (\g t e -> When g Nothing LapseRevert t e) <$> genGuard <*> few (n `div` 3) (genItem (n `div` 3)) <*> few (n `div` 3) (genItem (n `div` 3)))
        ]

genStep :: Gen StepI
genStep = do
  o <- genOp
  d <- elements [Forward, Inverse]
  pure (step o) {stepDirection = d}

genOp :: Gen Op
genOp = do
  i <- genName
  k <- elements [Owned, Region, Modified]
  s <- genName
  pure (op i [entry k ("file:/" <> s)])

genGuard :: Gen Guard
genGuard = guard <$> genName <*> elements [Yes, No, Unknown]

genName :: Gen T.Text
genName = T.pack <$> listOf1 (elements ['a' .. 'f'])

few :: Int -> Gen a -> Gen [a]
few n g = choose (0, min 4 (max 0 n)) >>= \k -> vectorOf k g
