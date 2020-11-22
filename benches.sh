#!/bin/zsh

set -e

cargo build --release

# TODO: back this up in a sqlite database or something so we can track progress?
echo "" > timings.txt

( time ( ./target/release/bf_interpreter ./input/mandelbrot.b opt > out.mandelbrot.txt ) ) 2>> timings.txt
( time ( ./target/release/bf_interpreter ./input/hanoi.b opt > out.hanoi.txt ) ) 2>> timings.txt
( time ( ./target/release/bf_interpreter ./input/long.b opt > out.long.txt ) ) 2>> timings.txt
( time ( echo 13333333333337 | ./target/release/bf_interpreter ./input/factor.b opt > out.factor.txt ) ) 2>> timings.txt

cat timings.txt
