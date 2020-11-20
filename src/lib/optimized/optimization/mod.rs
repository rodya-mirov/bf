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
        if let AST::Loop { ref mut elements } = cmd {
            total_removed += const_loop_remove(elements);
        }
    }

    fn only_data(cmds: &[AST]) -> (bool, HashMap<isize, u8>) {
        let mut offsets: HashMap<isize, u8> = HashMap::new();

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
                            return (false, offsets);
                        }
                    }
                }
                AST::Loop { .. } => {
                    return (false, offsets);
                }
                AST::ShiftDataPtr { .. } => {
                    return (false, offsets);
                }
                AST::ReadByte | AST::WriteByte => {
                    return (false, offsets);
                }
            }
        }

        (true, offsets)
    }

    let old = std::mem::replace(cmds, Vec::new());

    for mut cmd in old {
        if let AST::Loop { ref mut elements } = cmd {
            let (is_const, mut offsets) = only_data(elements);
            if is_const {
                if !offsets.contains_key(&0) {
                    // TODO: we could I guess handle this? Add an 'infinite loop' instruction or something
                    // It also is sort of unsound; if the start is 0, it's fine; which is important
                    // for (e.g.) the header comment in one of the test programs
                    // TODO: this is actually wrong; it should be "if S != 0, infinite loop"
                    panic!("Unhandled: infinite loop which does nothing");
                }

                if offsets.len() == 1 {
                    cmds.push(AST::ModData {
                        kind: DatamodKind::SetData { amount: 0 },
                        dp_offset: 0,
                    });
                    total_removed += 1;
                } else {
                    let zero_offset = offsets.remove(&0).unwrap();
                    println!(
                        "Found a const loop with offsets {:?}, zero offset {}, which should be an addition, which I could not kill",
                        offsets, zero_offset
                    );
                    cmds.push(cmd);
                }
            } else {
                cmds.push(cmd);
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
        if let AST::Loop { ref mut elements } = cmd {
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
                if let AST::ModData {
                    kind: second_kind,
                    dp_offset: second_dp_offset,
                } = cmd
                {
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
                } else {
                    cmds.push(acc);
                    accumulator = Some(cmd);
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
            AST::Loop { .. } => {
                cmds.push(acc);
                accumulator = Some(cmd);
            }
            AST::ReadByte | AST::WriteByte => {
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
        if let AST::Loop { ref mut elements } = cmd {
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

    fn maybe_swap(first: &mut AST, second: &mut AST) -> usize {
        let mut swap = false;
        match first {
            AST::WriteByte | AST::ReadByte | AST::Loop { .. } => {}
            AST::ModData { kind: _, dp_offset } => match second {
                AST::ModData {
                    kind: _,
                    dp_offset: second_offset,
                } => {
                    if dp_offset > second_offset {
                        swap = true;
                    }
                }
                _ => {}
            },
            AST::ShiftDataPtr { amount: shift_amount } => match second {
                AST::ModData { kind, dp_offset } => {
                    *second = AST::ModData {
                        kind: *kind,
                        dp_offset: *dp_offset + *shift_amount,
                    };
                    swap = true;
                }
                _ => {}
            },
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
            // The slice has length two, but the rust compiler doesn't (yet) know how to deal with that
            unreachable!()
        }
    }

    changed
}
