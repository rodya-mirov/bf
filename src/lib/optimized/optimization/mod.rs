use std::collections::HashMap;

use crate::optimized::DatamodKind;

use super::AST;
use crate::optimized::optimization::data_usage::{DataUsage, DataUsageTracker};

pub(crate) fn optimize(cmds: &mut Vec<AST>) {
    let mut step = 0;

    loop {
        let step_count = opt_step(cmds);

        println!("Step {} did {} changes.\n", step, step_count);

        if step_count == 0 {
            break;
        }

        step += 1;
    }
}

fn opt_step(cmds: &mut Vec<AST>) -> usize {
    let swap = sort_commands(cmds);
    println!("Swapped {} commands total", swap);

    let coll = collapse_consecutive(cmds);
    println!("Collapse {} consecutive pure commands total", coll);

    let deloop = const_loop_remove(cmds);
    println!("Killed {} const loops!", deloop);

    let simulate_removal = run_simulation(cmds);
    println!("Killed {} instructions by simulation.", simulate_removal);

    let one_step_loops = one_step_loops(cmds);
    println!("Killed {} instructions by one-step-loop simulation", one_step_loops);

    swap + coll + deloop + simulate_removal + one_step_loops
}

mod sim_state {
    use crate::optimized::DatamodKind;
    use std::collections::HashMap;
    use std::fmt;

    #[derive(Copy, Clone, Eq, PartialEq)]
    pub enum DataState {
        Unknown,        // result of ReadByte, or any static analysis we simply do not wish to do
        UnknownNonzero, // kind of hacky? but really common
        Known(u8),
    }

    impl fmt::Debug for DataState {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                DataState::Unknown => write!(f, "?"),
                DataState::UnknownNonzero => write!(f, "!0"),
                DataState::Known(v) => write!(f, "{}", v),
            }
        }
    }

    pub struct SimState {
        data: HashMap<isize, DataState>,
        def_value: DataState,
        dp: isize,
        wipes: usize,
    }

    #[derive(Clone)]
    pub struct BranchMarker(usize);

    impl SimState {
        pub fn new(def_value: DataState) -> Self {
            SimState {
                data: HashMap::new(),
                def_value,
                dp: 0,
                wipes: 0,
            }
        }

        pub fn make_branch(&self) -> (SimState, BranchMarker) {
            let branch = SimState {
                data: self.data.clone(),
                def_value: self.def_value,
                dp: self.dp,
                wipes: self.wipes,
            };
            (branch, BranchMarker(self.wipes))
        }

        pub fn merge_divergent(&mut self, branch: SimState, branch_marker: BranchMarker) {
            let BranchMarker(wipes_at_split) = branch_marker;

            if self.wipes != wipes_at_split || branch.wipes != wipes_at_split {
                // No intersection can occur because we've lost track of dp entirely
                self.clear_knowledge();
                return;
            }

            if self.def_value != branch.def_value {
                println!(
                    "Default values differ ({:?} vs {:?}), not sure how this occurred",
                    self.def_value, branch.def_value
                );
                self.clear_knowledge();
                return;
            }

            if self.dp != branch.dp {
                println!("Merging: dp differs, this is solveable but I didn't do it yet");
                self.clear_knowledge();
                return;
            }

            let old_data: HashMap<isize, DataState> = std::mem::replace(&mut self.data, HashMap::new());
            for (k, v) in old_data {
                if branch.data.get(&k) == Some(&v) {
                    self.data.insert(k, v);
                }
            }
        }

        pub fn get_data(&self, ind: isize) -> DataState {
            let ind = self.dp + ind;
            self.data.get(&ind).copied().unwrap_or(self.def_value)
        }

        pub fn set_data(&mut self, ind: isize, val: DataState) {
            let ind = self.dp + ind;
            self.data.insert(ind, val);
        }

        pub fn shift_ptr(&mut self, shift: isize) {
            self.dp += shift;
        }

        pub(crate) fn process_mod_data(&mut self, kind: DatamodKind, dp_offset: isize) {
            match kind {
                DatamodKind::SetData { amount } => self.set_data(dp_offset, DataState::Known(amount)),
                DatamodKind::AddData { amount: 0 } => {}
                DatamodKind::AddData { amount } => match self.get_data(dp_offset) {
                    DataState::Unknown | DataState::UnknownNonzero => {
                        self.set_data(dp_offset, DataState::Unknown);
                    }
                    DataState::Known(old) => {
                        self.set_data(dp_offset, DataState::Known(u8::wrapping_add(old, amount)));
                    }
                },
            }
        }

        pub fn process_combine_data(&mut self, source_dp_offset: isize, target_dp_offset: isize, source_amt_mult: u8) {
            if source_amt_mult == 0 {
                return;
            }

            match (self.get_data(source_dp_offset), self.get_data(target_dp_offset)) {
                (DataState::Known(0), _) => {}
                (DataState::Known(a), DataState::Known(b)) => {
                    let new_val = u8::wrapping_add(b, u8::wrapping_mul(a, source_amt_mult));
                    self.set_data(target_dp_offset, DataState::Known(new_val));
                }
                (DataState::Unknown, _) | (DataState::UnknownNonzero, _) | (_, DataState::Unknown) | (_, DataState::UnknownNonzero) => {
                    self.set_data(target_dp_offset, DataState::Unknown)
                }
            }
        }

        /// Clear everything we know about the state
        pub fn clear_knowledge(&mut self) {
            self.data.clear();
            // Everything is relative anyway; dp is only maintained to address into the known datastores
            self.dp = 0;
            self.def_value = DataState::Unknown;
            self.wipes += 1;
        }
    }

    impl fmt::Debug for SimState {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{{ default: {:?}, [", self.def_value)?;

            let mut first = true;
            let mut to_write = self.data.iter().filter(|(_k, v)| **v != self.def_value).collect::<Vec<_>>();
            to_write.sort_by_key(|(k, _)| **k);

            for (k, v) in to_write {
                if first {
                    first = false;
                } else {
                    write!(f, ", ")?;
                }
                write!(f, "{}: {:?}", k, &v)?;
            }

            write!(f, "]}}")
        }
    }
}

fn one_step_loops(cmds: &mut Vec<AST>) -> usize {
    use sim_state::{DataState, SimState};

    fn one_step_loops_ctx(old: Vec<AST>, state: &mut SimState) -> (Vec<AST>, usize) {
        let mut removed = 0;

        let mut cmds = Vec::new();

        for cmd in old {
            match cmd {
                AST::Loop {
                    cond_dp_offset,
                    elements,
                    known_to_be_nontrivial,
                } => {
                    let mut inner_state = SimState::new(DataState::Unknown);
                    inner_state.set_data(cond_dp_offset, DataState::UnknownNonzero);

                    let (mut new_elements, inner_removed) = one_step_loops_ctx(elements, &mut inner_state);
                    removed += inner_removed;

                    if inner_state.get_data(cond_dp_offset) == DataState::Known(0) {
                        if known_to_be_nontrivial {
                            println!(
                                    "Loop terminated in one iteration; branch always taken. Was loop CDO {}, hint {}, elements unchanged. Old state {:?}, inner state {:?}",
                                    cond_dp_offset, known_to_be_nontrivial, state, inner_state
                                );
                            new_elements.push(AST::AssertEquals {
                                dp_offset: cond_dp_offset,
                                val: 0,
                            });
                            cmds.append(&mut new_elements);
                            removed += 1;
                        } else {
                            println!(
                                    "Loop terminated in one iteration; turned to branch. Was loop CDO {}, hint {}, elements unchanged. Old state {:?}, inner state {:?}",
                                    cond_dp_offset, known_to_be_nontrivial, state, inner_state
                                );
                            new_elements.push(AST::AssertEquals {
                                dp_offset: cond_dp_offset,
                                val: 0,
                            });
                            cmds.push(AST::IfNonZero {
                                elements: new_elements,
                                cond_dp_offset,
                            });
                            removed += 1;
                        }
                    } else {
                        cmds.push(AST::Loop {
                            known_to_be_nontrivial,
                            cond_dp_offset,
                            elements: new_elements,
                        });
                    }

                    state.clear_knowledge();
                    state.set_data(cond_dp_offset, DataState::Known(0));
                }
                AST::IfNonZero { elements, cond_dp_offset } => {
                    match state.get_data(cond_dp_offset) {
                        DataState::Known(0) => {
                            println!("Deleted a branch (not executed)");
                            removed += 1 + elements.len();
                        }
                        DataState::Known(_) | DataState::UnknownNonzero => {
                            // Improvement was made, no need to analyze it now, we'll get it next time
                            println!("Deleted a branch (executed). CDO {}, State {:?}", cond_dp_offset, state);
                            removed += 1;
                            let (mut new_elements, inner_removed) = one_step_loops_ctx(elements, state);
                            removed += inner_removed;
                            cmds.append(&mut new_elements);
                        }
                        DataState::Unknown => {
                            // Walking the interior of the branch to see what we can do
                            let (mut branch_state, marker) = state.make_branch();

                            branch_state.set_data(cond_dp_offset, DataState::UnknownNonzero);

                            let (new_elements, inner_removed) = one_step_loops_ctx(elements, &mut branch_state);
                            cmds.push(AST::IfNonZero {
                                cond_dp_offset,
                                elements: new_elements,
                            });
                            removed += inner_removed;

                            state.merge_divergent(branch_state, marker);
                        }
                    }
                }
                AST::ShiftLoop {
                    known_to_be_nontrivial: _,
                    cond_dp_offset,
                    dp_shift: _,
                } => {
                    state.clear_knowledge();
                    state.set_data(cond_dp_offset, DataState::Known(0));
                    cmds.push(cmd);
                }
                AST::InfiniteLoop => {
                    cmds.push(cmd);
                }
                AST::ShiftDataPtr { amount } => {
                    state.shift_ptr(amount);
                    cmds.push(cmd);
                }
                AST::ModData { kind, dp_offset } => {
                    state.process_mod_data(kind, dp_offset);
                    cmds.push(cmd);
                }
                AST::CombineData {
                    source_dp_offset,
                    target_dp_offset,
                    source_amt_mult,
                } => {
                    state.process_combine_data(source_dp_offset, target_dp_offset, source_amt_mult);
                    cmds.push(cmd);
                }
                AST::ReadByte { dp_offset } => {
                    state.set_data(dp_offset, DataState::Unknown);
                    cmds.push(cmd);
                }
                AST::WriteByte { .. } => {
                    cmds.push(cmd);
                }
                AST::WriteConst { .. } => {
                    cmds.push(cmd);
                }
                AST::AssertEquals { dp_offset, val } => {
                    state.set_data(dp_offset, DataState::Known(val));
                    cmds.push(cmd);
                }
            }
        }

        (cmds, removed)
    }

    let old = std::mem::replace(cmds, Vec::new());

    let (new_cmds, removed) = one_step_loops_ctx(old, &mut SimState::new(DataState::Known(0)));
    *cmds = new_cmds;
    removed
}

// Do a one-pass simulation to see if we can use const analysis to eliminate branches, etc.
// This is NOT gonna just be a "run the thing at compile time" situation because I consider that
// cheating; this will be an O(n) operation where n is cmds.len(); we just sweep through and anything
// we can sort of determine in advance, we collapse
fn run_simulation(cmds: &mut Vec<AST>) -> usize {
    use sim_state::{DataState, SimState};

    fn run_simulation_ctx(cmds: &mut Vec<AST>, state: &mut SimState) -> usize {
        let mut removed = 0; // or simplified, or whatever

        let old = std::mem::replace(cmds, Vec::new());

        for cmd in old {
            match cmd {
                AST::IfNonZero { cond_dp_offset, elements } => {
                    match state.get_data(cond_dp_offset) {
                        DataState::Unknown => {
                            let cmd = AST::IfNonZero { cond_dp_offset, elements };
                            let usage = track_usage(&cmd);
                            cmds.push(cmd);

                            match usage {
                                DataUsage::DataTracked { dp_shift: 0, data_mods } => {
                                    for m in data_mods {
                                        state.set_data(m, DataState::Unknown);
                                    }
                                }
                                DataUsage::DataTracked { dp_shift, data_mods } => {
                                    println!("If statement has conditional shift, but it's unhandled; if you see this message, cover this case: shift: {}, mods: {:?}", dp_shift, data_mods);
                                    state.clear_knowledge();
                                }
                                _ => state.clear_knowledge(),
                            }
                        }
                        DataState::Known(0) => {
                            println!("Eliminated branch {:?} (not executed)", elements);
                            removed += 1;
                        }
                        DataState::UnknownNonzero | DataState::Known(_) => {
                            println!("Eliminated branch (executed)");
                            // successive passes will manage this? I guess
                            for elt in elements {
                                cmds.push(elt);
                            }
                            state.clear_knowledge();
                            removed += 1;
                        }
                    }
                }
                AST::ShiftDataPtr { amount } => {
                    state.shift_ptr(amount);
                    cmds.push(cmd);
                }
                AST::ShiftLoop { .. } => {
                    cmds.push(cmd);
                    state.clear_knowledge();
                    state.set_data(0, DataState::Known(0));
                }
                AST::Loop {
                    cond_dp_offset,
                    elements,
                    mut known_to_be_nontrivial,
                } => {
                    let keep_loop: bool;

                    match state.get_data(cond_dp_offset) {
                        DataState::Known(0) => {
                            println!("Eliminated loop (not executed)");
                            removed += 1;
                            keep_loop = false;
                        }
                        DataState::Known(_) | DataState::UnknownNonzero => {
                            if known_to_be_nontrivial {
                                println!(
                                    "Gave up on a loop, it already had the hint. State: {:?}, Elts: {:#?}",
                                    state, elements
                                );
                                keep_loop = true;
                            } else {
                                println!("Gave up on a loop, but emitted a 'will be executed' hint");
                                // not really removed, but at least simplified / improved?
                                keep_loop = true;
                                known_to_be_nontrivial = true;
                                removed += 1;
                            }
                        }
                        DataState::Unknown => {
                            println!("Gave up on a loop, no hint could be emitted anyway");
                            keep_loop = true;
                        }
                    }

                    if keep_loop {
                        let cmd = AST::Loop {
                            known_to_be_nontrivial,
                            elements,
                            cond_dp_offset,
                        };
                        let usage = track_usage(&cmd);
                        cmds.push(cmd);

                        if let DataUsage::DataTracked { dp_shift: 0, data_mods } = usage {
                            for m in data_mods {
                                state.set_data(m, DataState::Unknown);
                            }
                        } else {
                            state.clear_knowledge();
                        }
                    }
                    state.set_data(cond_dp_offset, DataState::Known(0));
                }
                AST::ReadByte { dp_offset } => {
                    state.set_data(dp_offset, DataState::Unknown);
                    cmds.push(AST::ReadByte { dp_offset });
                }
                AST::WriteByte { dp_offset } => match state.get_data(dp_offset) {
                    DataState::Unknown | DataState::UnknownNonzero => {
                        cmds.push(AST::WriteByte { dp_offset });
                    }
                    DataState::Known(val) => {
                        removed += 1;
                        cmds.push(AST::WriteConst { out: val });
                    }
                },
                AST::WriteConst { .. } => {
                    cmds.push(cmd);
                }
                AST::CombineData {
                    source_dp_offset,
                    target_dp_offset,
                    source_amt_mult,
                } => {
                    if let DataState::Known(old) = state.get_data(source_dp_offset) {
                        println!("Combine turned to add"); // if it's settable it'll be found on the next pass
                        removed += 1;
                        cmds.push(AST::ModData {
                            dp_offset: target_dp_offset,
                            kind: DatamodKind::AddData {
                                amount: u8::wrapping_mul(source_amt_mult, old),
                            },
                        });

                        state.process_combine_data(source_dp_offset, target_dp_offset, source_amt_mult);
                    } else {
                        state.process_combine_data(source_dp_offset, target_dp_offset, source_amt_mult);

                        let end_data = state.get_data(target_dp_offset);

                        if let DataState::Known(amount) = end_data {
                            println!("Combine turned to set");
                            removed += 1;
                            cmds.push(AST::ModData {
                                dp_offset: target_dp_offset,
                                kind: DatamodKind::SetData { amount },
                            });
                        } else {
                            cmds.push(cmd);
                        }
                    }
                }
                AST::ModData { kind, dp_offset } => {
                    let start_data = state.get_data(dp_offset);

                    state.process_mod_data(kind, dp_offset);

                    let end_data = state.get_data(dp_offset);

                    if let DataState::Known(amount) = end_data {
                        if start_data == end_data {
                            println!("No-op mod data deleted");
                            removed += 1;
                        } else {
                            let new_cmd = AST::ModData {
                                kind: DatamodKind::SetData { amount },
                                dp_offset,
                            };
                            match kind {
                                DatamodKind::SetData { amount: _ } => {}
                                DatamodKind::AddData { amount: _ } => {
                                    removed += 1;
                                }
                            }
                            cmds.push(new_cmd);
                        }
                    } else {
                        cmds.push(cmd);
                    }
                }
                _ => {
                    state.clear_knowledge();
                    println!("Gave up on {:?}", cmd);
                    cmds.push(cmd);
                }
            }
        }

        removed
    }

    let mut state = SimState::new(DataState::Known(0));
    run_simulation_ctx(cmds, &mut state)
}

// the result of "a, then b" on the same offset
fn collapse_kinds(a: DatamodKind, b: DatamodKind) -> DatamodKind {
    match b {
        DatamodKind::SetData { amount } => DatamodKind::SetData { amount },
        DatamodKind::AddData { amount: b_amt } => match a {
            DatamodKind::SetData { amount } => DatamodKind::SetData {
                amount: u8::wrapping_add(amount, b_amt),
            },
            DatamodKind::AddData { amount } => DatamodKind::AddData {
                amount: u8::wrapping_add(amount, b_amt),
            },
        },
    }
}

// Precondition: everything is sorted and collapsed
fn const_loop_remove(cmds: &mut Vec<AST>) -> usize {
    // first, apply recursively; so we have loops that could be removed, but not loops that
    // have removable loops as elements

    let mut total_removed = 0;

    for cmd in cmds.iter_mut() {
        if let AST::Loop {
            ref mut elements,
            cond_dp_offset: _,
            known_to_be_nontrivial: _,
        } = cmd
        {
            total_removed += const_loop_remove(elements);
        }
    }

    // Ordered top to bottom; so if it contains Sets and Shifts it comes back as Shifts
    #[derive(Copy, Clone, Eq, PartialEq, Debug, Ord, PartialOrd)]
    enum NonConstResult {
        ComplexArithmetic,
        Shifts,
        InnerCond,
        InnerLoops,
        IO,
        InfiniteLoop,
    }

    fn only_data(cmds: &[AST]) -> Result<HashMap<isize, DatamodKind>, NonConstResult> {
        let mut offsets: HashMap<isize, DatamodKind> = HashMap::new();

        let mut running_error: Option<NonConstResult> = None;

        let mut update_err = |e| {
            if let Some(old) = running_error {
                running_error = Some(old.max(e));
            } else {
                running_error = Some(e);
            }
        };

        for cmd in cmds {
            match cmd {
                AST::ModData { kind, dp_offset } => {
                    let val = offsets.entry(*dp_offset).or_insert(DatamodKind::AddData { amount: 0 });
                    *val = collapse_kinds(*val, *kind);
                    if *val == (DatamodKind::AddData { amount: 0 }) {
                        offsets.remove(dp_offset);
                    }
                }
                AST::CombineData { .. } => {
                    update_err(NonConstResult::ComplexArithmetic);
                }
                AST::Loop { .. } => {
                    update_err(NonConstResult::InnerLoops);
                }
                AST::ShiftLoop { .. } => {
                    update_err(NonConstResult::InnerLoops);
                }
                AST::IfNonZero { .. } => {
                    update_err(NonConstResult::InnerCond);
                }
                AST::ShiftDataPtr { .. } => {
                    update_err(NonConstResult::Shifts);
                }
                AST::ReadByte { .. } | AST::WriteByte { .. } | AST::WriteConst { .. } => {
                    update_err(NonConstResult::IO);
                }
                AST::InfiniteLoop => {
                    update_err(NonConstResult::InfiniteLoop);
                }
                AST::AssertEquals { .. } => {}
            }
        }

        match running_error {
            Some(e) => Err(e),
            None => Ok(offsets),
        }
    }

    let old = std::mem::replace(cmds, Vec::new());

    for mut cmd in old {
        if let AST::Loop {
            ref mut elements,
            cond_dp_offset,
            known_to_be_nontrivial,
        } = cmd
        {
            match only_data(elements) {
                Ok(mut offsets) => {
                    if !offsets.contains_key(&cond_dp_offset) {
                        if known_to_be_nontrivial {
                            println!("Emitted IL");
                            cmds.push(AST::InfiniteLoop);
                        } else {
                            println!("Emitted cond IL");
                            cmds.push(AST::IfNonZero {
                                cond_dp_offset,
                                elements: vec![AST::InfiniteLoop],
                            });
                        }
                    }

                    if offsets.len() == 1 {
                        // TODO BUG: this is actually wrong; it should be "if the 2-ness of offsets[0] is <= the 2-ness of S, set 0; else inf loop"
                        // this won't actually detect an infinite loop, sadly

                        // Note we don't need the hint to eliminate the branch -- we know the offset
                        // is valid (assuming the loop is evaluable) so "if x != 0 { x = 0 }" is
                        // more simply stated as "x = 0"
                        cmds.push(AST::ModData {
                            kind: DatamodKind::SetData { amount: 0 },
                            dp_offset: cond_dp_offset,
                        });
                        total_removed += 1;
                    } else {
                        let zero_offset = offsets.remove(&cond_dp_offset).unwrap();

                        // in this case it literally just iterates exactly data[dp] times, so it's really easy
                        // this seems like a weird special case but it's really common
                        if let DatamodKind::AddData { amount } = zero_offset {
                            if amount != 1 && amount != u8::max_value() {
                                // I mean this literally never happens in my benchmark???
                                println!(
                                    "Found a const loop with offsets {:?}, zero offset {:?}, which should be solvable, but which I could not kill",
                                    offsets, zero_offset
                                );
                                cmds.push(cmd);
                            } else {
                                // The number of loop repetitions is the value of zero, times this number
                                let reps_mult = {
                                    if amount == u8::max_value() {
                                        1
                                    } else if amount == 1 {
                                        u8::max_value()
                                    } else {
                                        unreachable!()
                                    }
                                };

                                let mut loop_adds = Vec::new();
                                for (target_dp_offset, kind) in offsets {
                                    match kind {
                                        DatamodKind::AddData { amount: base_amt_mult } => {
                                            loop_adds.push(AST::CombineData {
                                                source_dp_offset: cond_dp_offset,
                                                target_dp_offset,
                                                // Each repetition adds base_amt_mult to the target
                                                // We repeat this operation resp_mult * source_data times
                                                // So equivalently target_data += base_amt_mult * source_data * reps_mult
                                                // This is only confusing because everything has overflow, but modular + and * work so it's fine
                                                source_amt_mult: u8::wrapping_mul(reps_mult, base_amt_mult),
                                            });
                                        }
                                        // Disappointingly never seems to happen?
                                        DatamodKind::SetData { amount: target_set_amt } => {
                                            loop_adds.push(AST::ModData {
                                                kind: DatamodKind::SetData { amount: target_set_amt },
                                                dp_offset: target_dp_offset,
                                            });
                                        }
                                    }
                                }

                                loop_adds.push(AST::ModData {
                                    kind: DatamodKind::SetData { amount: 0 },
                                    dp_offset: cond_dp_offset,
                                });

                                total_removed += 1;

                                if known_to_be_nontrivial {
                                    for inner in loop_adds {
                                        cmds.push(inner);
                                    }
                                } else {
                                    cmds.push(AST::IfNonZero {
                                        cond_dp_offset,
                                        elements: loop_adds,
                                    });
                                }
                            }
                        } else if let DatamodKind::SetData { amount } = zero_offset {
                            // then this is actually a single execution (if amount is zero) or an infinite loop (if it's not)
                            let mut loop_adds = Vec::new();
                            if amount != 0 {
                                loop_adds.push(AST::InfiniteLoop);
                            } else {
                                for (target_dp_offset, kind) in offsets {
                                    loop_adds.push(AST::ModData {
                                        kind,
                                        dp_offset: target_dp_offset,
                                    });
                                }
                                loop_adds.push(AST::ModData {
                                    kind: DatamodKind::SetData { amount: 0 },
                                    dp_offset: cond_dp_offset,
                                });
                            }
                            if known_to_be_nontrivial {
                                loop_adds.into_iter().for_each(|e| cmds.push(e));
                            } else {
                                cmds.push(AST::IfNonZero {
                                    cond_dp_offset,
                                    elements: loop_adds,
                                });
                            }
                        } else {
                            // I mean this literally never happens in my benchmark???
                            println!(
                                "Found a const loop with offsets {:?}, zero offset {:?}, which should be solvable, but which I could not kill",
                                offsets, zero_offset
                            );
                            cmds.push(cmd);
                        }
                    }
                }
                Err(_reason) => {
                    if elements.is_empty() {
                        println!("Emitted infinite loop (empty loop)");
                        cmds.push(AST::IfNonZero {
                            elements: vec![AST::InfiniteLoop],
                            cond_dp_offset,
                        });
                        total_removed += 1;
                    } else if elements.len() == 1 {
                        match elements.get(0).unwrap() {
                            AST::ShiftDataPtr { amount } => {
                                cmds.push(AST::ShiftLoop {
                                    dp_shift: *amount,
                                    known_to_be_nontrivial,
                                    cond_dp_offset,
                                });
                                total_removed += 1;
                            }
                            other => {
                                println!("Singleton loop, non eliminable: {:?}", other);
                            }
                        }
                    } else {
                        // println!("Could not destroy loop for reason {:?}", reason);
                        cmds.push(cmd);
                    }
                }
            }
        } else {
            cmds.push(cmd);
        }
    }

    total_removed
}

fn collapse_consecutive(cmds: &mut Vec<AST>) -> usize {
    if cmds.is_empty() {
        return 0;
    }

    // the idea of this optimization is to just collapse consecutive commands that can be
    // expressed as a single command; eg. Add 3, then Add 2, becomes Add 5

    let mut old = Vec::new();
    std::mem::swap(cmds, &mut old);

    let mut collapsed = 0;

    let mut accumulator: Option<AST> = None;

    // First, recursively apply to loops
    for cmd in old.iter_mut() {
        if let AST::Loop {
            ref mut elements,
            cond_dp_offset: _,
            known_to_be_nontrivial: _,
        } = cmd
        {
            collapsed += collapse_consecutive(elements);
        }
    }

    // Then for top-level, do any collapsing of consecutive "matching" terms
    for cmd in old {
        if accumulator.is_none() {
            accumulator = Some(cmd);
            continue;
        }

        let acc = accumulator.unwrap();

        match acc {
            AST::ModData { kind, dp_offset } => {
                match cmd {
                    AST::ModData {
                        kind: second_kind,
                        dp_offset: second_dp_offset,
                    } => {
                        if dp_offset == second_dp_offset {
                            let out_kind = match (kind, second_kind) {
                                (DatamodKind::AddData { amount: a }, DatamodKind::AddData { amount: b }) => DatamodKind::AddData {
                                    amount: u8::wrapping_add(a, b),
                                },
                                (DatamodKind::SetData { amount: a }, DatamodKind::AddData { amount: b }) => DatamodKind::SetData {
                                    amount: u8::wrapping_add(a, b),
                                },
                                (_, DatamodKind::SetData { amount }) => DatamodKind::SetData { amount },
                            };
                            accumulator = Some(AST::ModData { kind: out_kind, dp_offset });
                            collapsed += 1;
                        } else {
                            cmds.push(acc);
                            accumulator = Some(cmd);
                        }
                    }
                    AST::ReadByte { dp_offset: read_dpo } if read_dpo == dp_offset => {
                        // the read just overwrites
                        accumulator = Some(cmd);
                        collapsed += 1;
                    }
                    AST::InfiniteLoop => {
                        println!("Swallowed by IL");
                        accumulator = Some(AST::InfiniteLoop);
                        collapsed += 1;
                    }
                    _ => {
                        cmds.push(acc);
                        accumulator = Some(cmd);
                    }
                }
            }
            AST::ShiftDataPtr { amount } => match cmd {
                AST::ShiftDataPtr { amount: other_amount } => {
                    let new_amount = amount + other_amount;
                    if new_amount == 0 {
                        accumulator = None;
                        collapsed += 2;
                    } else {
                        accumulator = Some(AST::ShiftDataPtr { amount: new_amount });
                        collapsed += 1;
                    }
                }
                AST::InfiniteLoop => {
                    println!("Swallowed by IL");
                    accumulator = Some(AST::InfiniteLoop);
                    collapsed += 1;
                }
                _ => {
                    cmds.push(acc);
                    accumulator = Some(cmd);
                }
            },
            AST::CombineData {
                source_dp_offset,
                target_dp_offset,
                source_amt_mult,
            } => match cmd {
                AST::CombineData {
                    source_dp_offset: other_sdo,
                    target_dp_offset: other_tdo,
                    source_amt_mult: other_sam,
                } => {
                    if source_dp_offset == other_sdo && target_dp_offset == other_tdo {
                        accumulator = Some(AST::CombineData {
                            source_dp_offset,
                            target_dp_offset,
                            source_amt_mult: u8::wrapping_add(source_amt_mult, other_sam),
                        });
                    } else {
                        cmds.push(acc);
                        accumulator = Some(cmd);
                    }
                }
                AST::InfiniteLoop => {
                    println!("Swallowed by IL");
                    accumulator = Some(AST::InfiniteLoop);
                    collapsed += 1;
                }
                _ => {
                    cmds.push(acc);
                    accumulator = Some(cmd);
                }
            },
            // Infinite loops never terminate, so any following commands can be dropped
            AST::InfiniteLoop => {
                println!("Deleted command following an infinite loop");
                accumulator = Some(acc);
                collapsed += 1;
            }
            AST::Loop { cond_dp_offset, .. } | AST::ShiftLoop { cond_dp_offset, .. } => match cmd {
                AST::Loop {
                    cond_dp_offset: other_cdo, ..
                }
                | AST::ShiftLoop {
                    cond_dp_offset: other_cdo, ..
                } if other_cdo == cond_dp_offset => {
                    // If that is "while nonzero" on the same thing as this one, we can skip it
                    accumulator = Some(acc);
                    collapsed += 1;
                }
                _ => {
                    cmds.push(acc);
                    accumulator = Some(cmd);
                }
            },
            AST::ReadByte { .. } | AST::WriteByte { .. } | AST::WriteConst { .. } | AST::IfNonZero { .. } => {
                cmds.push(acc);
                accumulator = Some(cmd);
            }
            AST::AssertEquals { .. } => {
                cmds.push(acc);
                accumulator = Some(cmd);
            }
        }
    }

    if let Some(last) = accumulator {
        cmds.push(last);
    }

    collapsed
}

fn sort_commands(cmds: &mut [AST]) -> usize {
    for cmd in cmds.iter_mut() {
        if let AST::Loop {
            ref mut elements,
            cond_dp_offset: _,
            known_to_be_nontrivial: _,
        }
        | AST::IfNonZero {
            ref mut elements,
            cond_dp_offset: _,
        } = cmd
        {
            sort_commands(elements);
        }
    }

    let mut slice_len = cmds.len();

    let mut total_swaps = 0;
    while slice_len >= 2 {
        let local_swaps = sort_commands_step(&mut cmds[0..slice_len]);
        if local_swaps == 0 {
            return total_swaps;
        }
        total_swaps += local_swaps;
        slice_len -= 1;
    }

    total_swaps
}

// This is essentially one iteration of a bubblesort; because the "swap" actually alters the
// elements it's hard to be sure that something faster would still work
fn sort_commands_step(cmds: &mut [AST]) -> usize {
    if cmds.len() < 2 {
        return 0;
    }

    let mut changed = 0;

    // First read/write, then infiniteLoop, then modData, then addData, then shiftPtr
    // Loop is considered unswappable for now
    // Note that the order of IO operations is not swappable
    fn maybe_swap(first: &mut AST, second: &mut AST) -> usize {
        let mut swap = false;
        match first {
            AST::WriteByte { .. }
            | AST::WriteConst { .. }
            | AST::ReadByte { .. }
            | AST::Loop { .. }
            | AST::ShiftLoop { .. }
            | AST::IfNonZero { .. }
            | AST::AssertEquals { .. }
            | AST::InfiniteLoop => {}
            AST::ModData { kind: _, dp_offset } => match second {
                AST::InfiniteLoop => swap = true,
                AST::ModData {
                    kind: _,
                    dp_offset: second_offset,
                } => {
                    if dp_offset > second_offset {
                        swap = true;
                    }
                }
                AST::ReadByte { dp_offset: io_offset } if io_offset != dp_offset => {
                    swap = true;
                }
                AST::WriteByte { dp_offset: io_offset } if io_offset != dp_offset => {
                    swap = true;
                }
                _ => {}
            },
            AST::CombineData {
                source_dp_offset,
                target_dp_offset,
                source_amt_mult: _,
            } => match second {
                AST::InfiniteLoop => swap = true,
                AST::CombineData {
                    source_dp_offset: other_sdo,
                    target_dp_offset: other_tdo,
                    source_amt_mult: _,
                } => {
                    if source_dp_offset == other_sdo {
                        // pretty simple, if they're the same source, order doesn't matter; subsort by target
                        if target_dp_offset > other_tdo {
                            swap = true;
                        }
                    } else if target_dp_offset == other_tdo {
                        // if they have the same target, order doesn't matter; sort by source
                        if source_dp_offset > other_sdo {
                            swap = true;
                        }
                    } else if source_dp_offset != other_tdo && target_dp_offset != other_sdo {
                        // if they have nothing to do with each other, order doesn't matter; sort by source
                        if source_dp_offset > other_sdo {
                            swap = true;
                        }
                    }
                }
                AST::ModData { kind: _, dp_offset } => {
                    // we want complex things after simple things (I guess?) but not everything swaps easily
                    // basically A += B; C += x can be swapped so long as C and B aren't pointing to the same place
                    if source_dp_offset != dp_offset {
                        swap = true;
                    }
                }
                AST::ReadByte { dp_offset: io_offset } if io_offset != source_dp_offset && io_offset != target_dp_offset => {
                    swap = true;
                }
                AST::WriteByte { dp_offset: io_offset } if io_offset != source_dp_offset && io_offset != target_dp_offset => {
                    swap = true;
                }
                _ => {}
            },
            AST::ShiftDataPtr { amount: shift_amount } => {
                if !matches!(second, AST::ShiftDataPtr {..}) {
                    shift_command(second, *shift_amount);
                    swap = true;
                }
            }
        }

        if swap {
            std::mem::swap(first, second);
            1
        } else {
            0
        }
    }

    for i in 0..cmds.len() - 1 {
        if let [ref mut a, ref mut b] = cmds[i..i + 2] {
            changed += maybe_swap(a, b);
        } else {
            // The slice has length two, but the rust compiler doesn't (yet) know how to know that
            unreachable!()
        }
    }

    changed
}

fn shift_command(cmd: &mut AST, dp_shift: isize) {
    match cmd {
        AST::Loop {
            ref mut cond_dp_offset,
            ref mut elements,
            known_to_be_nontrivial: _,
        } => {
            *cond_dp_offset += dp_shift;
            elements.iter_mut().for_each(|e| shift_command(e, dp_shift));
        }
        AST::ShiftLoop {
            ref mut cond_dp_offset, ..
        } => {
            *cond_dp_offset += dp_shift;
        }
        AST::ShiftDataPtr { amount: _ } => {
            // It's fine, no need to shift, they commute
        }
        AST::ModData {
            kind: _,
            ref mut dp_offset,
        } => {
            *dp_offset += dp_shift;
        }
        AST::CombineData {
            source_dp_offset,
            target_dp_offset,
            source_amt_mult: _,
        } => {
            *source_dp_offset += dp_shift;
            *target_dp_offset += dp_shift;
        }
        AST::ReadByte { dp_offset } => {
            *dp_offset += dp_shift;
        }
        AST::WriteByte { dp_offset } => {
            *dp_offset += dp_shift;
        }
        AST::IfNonZero {
            ref mut cond_dp_offset,
            ref mut elements,
        } => {
            *cond_dp_offset += dp_shift;
            elements.iter_mut().for_each(|e| shift_command(e, dp_shift));
        }
        AST::WriteConst { .. } => {
            // It's fine, it's done
        }
        AST::InfiniteLoop => {
            // It's fine, it's done
        }
        AST::AssertEquals { ref mut dp_offset, val: _ } => {
            *dp_offset += dp_shift;
        }
    }
}

mod data_usage {
    use std::collections::HashSet;

    pub struct DataUsageTracker(DataUsage);

    pub enum DataUsage {
        // Data pointer was lost, usually due to control flow confusion, so
        // no detailed results can be shown. Note that infinite loops / OOBs
        // don't "use data" or "lose dp"
        DpLost,
        DataTracked { dp_shift: isize, data_mods: HashSet<isize> },
    }

    impl DataUsageTracker {
        pub fn new() -> Self {
            DataUsageTracker(DataUsage::DataTracked {
                dp_shift: 0,
                data_mods: HashSet::new(),
            })
        }

        pub fn shift(&mut self, shift_amount: isize) {
            match &mut self.0 {
                DataUsage::DpLost => {}
                DataUsage::DataTracked {
                    ref mut dp_shift,
                    ref mut data_mods,
                } => {
                    *dp_shift += shift_amount;
                    *data_mods = data_mods.iter().map(|dp| dp + shift_amount).collect()
                }
            }
        }

        pub fn complete(self) -> DataUsage {
            self.0
        }

        // This is data (potentially) modified
        pub fn data_used(&mut self, dp_offset: isize) {
            match &mut self.0 {
                DataUsage::DpLost => {}
                DataUsage::DataTracked {
                    dp_shift: _,
                    ref mut data_mods,
                } => {
                    data_mods.insert(dp_offset);
                }
            }
        }

        pub fn lose_dp(&mut self) {
            self.0 = DataUsage::DpLost;
        }
    }
}

fn track_usage(cmd: &AST) -> DataUsage {
    fn track_usage_step(cmd: &AST, tracker: &mut DataUsageTracker) {
        match cmd {
            // For a loop or a branch; if there is no net dp shift inside
            // we can just say "well we altered these data points and that's all"
            AST::Loop {
                known_to_be_nontrivial: _,
                cond_dp_offset,
                ref elements,
            }
            | AST::IfNonZero {
                cond_dp_offset,
                ref elements,
            } => {
                tracker.data_used(*cond_dp_offset);

                let mut inside_tracker = DataUsageTracker::new();
                for elt in elements {
                    track_usage_step(elt, &mut inside_tracker);
                }

                match inside_tracker.complete() {
                    DataUsage::DpLost => {
                        tracker.lose_dp();
                    }
                    DataUsage::DataTracked { dp_shift, data_mods } => {
                        if dp_shift != 0 {
                            tracker.lose_dp();
                        }

                        for dm in data_mods {
                            tracker.data_used(dm);
                        }
                    }
                }
            }
            AST::ShiftLoop { .. } => {
                tracker.lose_dp();
            }
            AST::ShiftDataPtr { amount } => {
                tracker.shift(*amount);
            }
            AST::ModData { kind: _, dp_offset } => {
                tracker.data_used(*dp_offset);
            }
            AST::CombineData {
                source_dp_offset: _,
                target_dp_offset,
                source_amt_mult: _,
            } => {
                tracker.data_used(*target_dp_offset);
            }
            AST::ReadByte { dp_offset } => {
                tracker.data_used(*dp_offset);
            }
            AST::WriteByte { dp_offset } => {
                tracker.data_used(*dp_offset);
            }
            AST::InfiniteLoop => {}
            AST::WriteConst { .. } => {}
            AST::AssertEquals { dp_offset, val: _ } => {
                tracker.data_used(*dp_offset);
            }
        }
    }

    let mut tracker = DataUsageTracker::new();

    track_usage_step(cmd, &mut tracker);

    tracker.complete()
}
