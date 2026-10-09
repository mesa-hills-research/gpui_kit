-- | A module comment
module Main where

import Data.List (sortBy)

data Shape = Circle Double | Square Double deriving (Show)

area :: Shape -> Double
area (Circle r) = pi * r * r
area (Square s) = s * s

main :: IO ()
main = do
  let shapes = [Circle 1.0, Square 2]
  mapM_ (print . area) shapes
  putStrLn "done\n"
  if True then return () else pure ()
