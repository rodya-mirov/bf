TODO for v0.1 release:
--

 - [x] Scriptize benchmarks
 - [ ] Proper CLI interface:
    - [ ] Source file is a named arg
    - [ ] Optimized source output file / optimization spam is a named arg (default to omit)
    - [ ] Timing / Instruction count is a named arg (default to omit)
    - [ ] Script output is a named arg (default to stdout)
    - [ ] Script input is a named arg (default to stdin)
 - [ ] SQLite perf tracking (instruction counts, opt timing, run timing, with named runs and commit hashes)
 - [x] Cargo test with known input / output (integration tests, basically)
 
 
Available optimizations:
--

- (Minor) Can delete trailing top-level commands that aren't stateful
    e.g. any trailing PtrShift is just irrelevant
    
    Note that we shouldn't (e.g.) delete trailing AddData or something
    because we could lose an OutOfBounds error (sigh). Unlikely that
    there is a lot of gain hiding here, but it seems wasteful to leave
    it on the table. 

 
 - Simulation:

    - [x] Improvement: can work "relative to" an unknown dp offset
        with a relative state (use a HashMap instead of an array);
        this can't constantize as many things, but it can still do
        a lot of useful work. Every time we "lose" dp, we have to
        erase the state, but we can still keep going.

    - [ ] Improvement: we can have "constraints" on a datapoint instead
        of only known/unknown; useful examples:
         - At the inside of a loop, we can assume the loop datum is
            nonzero at the first point which can do some loop hinting
            and if-branch collapsing
    
    - [x] If we can't collapse a loop, but it's dp-neutral, just 
        "unknown" out every datapoint it touches and continue on
        
    - [x] If we can't collapse a loop, and it's not dp-neutral, just
        "unknown" out everything, and continue on
    
    - [x] Following a loop, we can assume the cond_dp is zero
    
    - [ ] Simulate a loop for ONE ITERATION; if it's guaranteed to terminate
        based on what we know going into it, replace it with an IfNonzero,
        or with a hint, just the code itself.
        
        - I think this does happen with the quine