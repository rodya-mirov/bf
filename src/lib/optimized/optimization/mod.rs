use std::collections::HashMap;

use super::AST;
use crate::optimized::DatamodKind;

pub(crate) fn optimize(cmds: &mut Vec<AST>) {
    let mut step = 0;

    loop {
        let step_count = opt_step(cmds);
        if step_count == 0 {
            break;
        }

        println!("Step {} did {} changes", step, step_count);
        step += 1;
    }
}

fn opt_step(cmds: &mut Vec<AST>) -> usize {
    let swap = sort_commands(cmds);
    let coll = collapse_consecutive(cmds);
    let deloop = const_loop_remove(cmds);

    println!("Swapped {} commands total", swap);
    println!("Collapse {} consecutive pure commands total", coll);
    println!("Killed {} const loops!", deloop);

    swap + coll + deloop
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
        } = cmd
        {
            total_removed += const_loop_remove(elements);
        }
    }

    // Ordered top to bottom; so if it contains Sets and Shifts it comes back as Shifts
    #[derive(Copy, Clone, Eq, PartialEq, Debug, Ord, PartialOrd)]
    enum NonConstResult {
        Sets,
        ComplexArithmetic,
        Shifts,
        InnerCond,
        InnerLoops,
        IO,
        InfiniteLoop,
    }

    fn only_data(cmds: &[AST]) -> Result<HashMap<isize, u8>, NonConstResult> {
        let mut offsets: HashMap<isize, u8> = HashMap::new();

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
                    match kind {
                        DatamodKind::AddData { amount } => {
                            let val = offsets.entry(*dp_offset).or_insert(0);
                            *val = val.wrapping_add(*amount);
                            if *val == 0 {
                                offsets.remove(dp_offset);
                            }
                        }
                        DatamodKind::SetData { amount: _ } => {
                            // TODO this is sort of a mess for now; Set can be fixed but it requires a conditional
                            return Err(NonConstResult::Sets);
                        }
                    }
                }
                AST::CombineData { .. } => {
                    update_err(NonConstResult::ComplexArithmetic);
                }
                AST::Loop { .. } => {
                    update_err(NonConstResult::InnerLoops);
                }
                AST::IfNonZero {..} => {
                    update_err(NonConstResult::InnerCond);
                }
                AST::ShiftDataPtr { .. } => {
                    update_err(NonConstResult::Shifts);
                }
                AST::ReadByte { .. } | AST::WriteByte { .. } => {
                    update_err(NonConstResult::IO);
                }
                AST::InfiniteLoop => {
                    update_err(NonConstResult::InfiniteLoop);
                }
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
        } = cmd
        {
            match only_data(elements) {
                Ok(mut offsets) => {
                    // the "is_const" means it has no jumps; note that due to recursive sorting,
                    // jumps can often be expunged, so it means "it has no net jumps and no inner
                    // loops or funny business"
                    if !offsets.contains_key(&cond_dp_offset) {
                        cmds.push(AST::IfNonZero {
                            elements: vec![AST::InfiniteLoop],
                            cond_dp_offset,
                        });
                    }

                    if offsets.len() == 1 {
                        // TODO: this is actually wrong; it should be "if the 2-ness of offsets[0] is <= the 2-ness of S, set 0; else inf loop"
                        cmds.push(AST::IfNonZero {
                            elements: vec![AST::ModData {
                                kind: DatamodKind::SetData { amount: 0 },
                                dp_offset: cond_dp_offset,
                            }],
                            cond_dp_offset,
                        });
                        total_removed += 1;
                    } else {
                        let zero_offset = offsets.remove(&cond_dp_offset).unwrap();

                        // in this case it literally just iterates exactly data[dp] times, so it's really easy
                        // this seems like a weird special case but it's really common
                        if zero_offset == u8::max_value() {
                            let mut loop_adds = Vec::new();
                            for (target_dp_offset, source_amt_mult) in offsets {
                                loop_adds.push(AST::CombineData {
                                    source_dp_offset: cond_dp_offset,
                                    target_dp_offset,
                                    source_amt_mult,
                                });
                            }
                            loop_adds.push(AST::ModData {
                                kind: DatamodKind::SetData { amount: 0 },
                                dp_offset: cond_dp_offset,
                            });

                            total_removed += 1;

                            cmds.push(AST::IfNonZero {
                                cond_dp_offset,
                                elements: loop_adds,
                            });
                        } else {
                            println!(
                                "Found a const loop with offsets {:?}, zero offset {}, which should be solvable, but which I could not kill",
                                offsets, zero_offset
                            );
                            cmds.push(cmd);
                        }
                    }
                }
                Err(reason) => {
                    println!("Could not destroy loop for reason {:?}", reason);
                    cmds.push(cmd);
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
                                (DatamodKind::AddData { amount: a }, DatamodKind::AddData { amount: b }) => {
                                    DatamodKind::AddData { amount: a + b }
                                }
                                (DatamodKind::SetData { amount: a }, DatamodKind::AddData { amount: b }) => {
                                    DatamodKind::SetData { amount: a + b }
                                }
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
                    _ => {
                        cmds.push(acc);
                        accumulator = Some(cmd);
                    }
                }
            }
            AST::ShiftDataPtr { amount } => {
                if let AST::ShiftDataPtr { amount: other_amount } = cmd {
                    let new_amount = amount + other_amount;
                    if new_amount == 0 {
                        accumulator = None;
                        collapsed += 2;
                    } else {
                        accumulator = Some(AST::ShiftDataPtr { amount: new_amount });
                        collapsed += 1;
                    }
                } else {
                    cmds.push(acc);
                    accumulator = Some(cmd);
                }
            }
            AST::CombineData {
                source_dp_offset,
                target_dp_offset,
                source_amt_mult,
            } => {
                if let AST::CombineData {
                    source_dp_offset: other_sdo,
                    target_dp_offset: other_tdo,
                    source_amt_mult: other_sam,
                } = cmd
                {
                    if source_dp_offset == other_sdo && target_dp_offset == other_tdo {
                        accumulator = Some(AST::CombineData {
                            source_dp_offset,
                            target_dp_offset,
                            source_amt_mult: source_amt_mult + other_sam,
                        });
                    } else {
                        cmds.push(acc);
                        accumulator = Some(cmd);
                    }
                } else {
                    cmds.push(acc);
                    accumulator = Some(cmd);
                }
            }
            AST::Loop { .. } | AST::ReadByte { .. } | AST::WriteByte { .. } | AST::InfiniteLoop | AST::IfNonZero { .. } => {
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
            AST::WriteByte { .. } | AST::ReadByte { .. } | AST::Loop { .. } | AST::IfNonZero { .. } | AST::InfiniteLoop => {}
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
            ref mut elements,
            ref mut cond_dp_offset,
        } => {
            *cond_dp_offset += dp_shift;
            shift_commands(elements, dp_shift);
        }
        AST::ShiftDataPtr { amount: _ } => {
            // nothing to shift here
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
            ref mut elements,
            ref mut cond_dp_offset,
        } => {
            shift_commands(elements, dp_shift);
            *cond_dp_offset += dp_shift;
        }
        AST::InfiniteLoop => {
            // it's fine
        }
    }
}

fn shift_commands(cmds: &mut [AST], dp_shift: isize) {
    for cmd in cmds.iter_mut() {
        shift_command(cmd, dp_shift);
    }
}
