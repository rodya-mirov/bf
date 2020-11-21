/// These are the "compiled instructions" which are to be used after parsing and optimizing.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum CompiledInstr {
    // Read the value data pointer; if zero, jump to target, otherwise increment ip
    JumpIfZero {
        cond_dp_offset: isize,
        target_ip: usize,
    },
    // Read the value data pointer; if nonzero, jump to target, otherwise increment ip
    JumpIfNonzero {
        cond_dp_offset: isize,
        target_ip: usize,
    },
    // Happens with bad code; interpreter can just bomb out if they want
    InfiniteLoop,
    // Happens with bad code; interpreter can just bomb out if they want
    AccessOutOfBounds,
    // Adds a given amount to the data pointer.
    AddPtr {
        amount: usize,
    },
    // Subtracts a given amount to the data pointer.
    SubPtr {
        amount: usize,
    },
    // Add the given amount to the byte at the data pointer. Note that due to wrapping,
    // we can (e.g.) subtract 1 by adding 255.
    // dp_offset means "add amount to byte at dp + dp_offset". Out of bounds writes are UB and
    // not handled (sorry).
    AddData {
        amount: u8,
        dp_offset: isize,
    },
    SetData {
        amount: u8,
        dp_offset: isize,
    },
    // data[dp + tdo] += data[dp + sdo] * sam
    AddTwoData {
        source_dp_offset: isize,
        target_dp_offset: isize,
        source_amt_mult: u8,
    },
    // Read a byte from stdin, or whatever IO method is configured
    ReadByte {
        dp_offset: isize,
    },
    // Write a byte to stdout, or whatever IO method is configured
    WriteByte {
        dp_offset: isize,
    },
    WriteConst {
        out: u8,
    },
}

/// Very similar to the compiled situation, but nested for the benefit of loop folding
/// This is structured for the benefit of compiling / optimizing; this is not the bytecode
/// format for the interpreter.
#[derive(Debug)]
pub(crate) enum AST {
    Loop {
        cond_dp_offset: isize,
        elements: Vec<AST>,
        // If this is true, it is known that it will be executed at least once
        // If this is false, nothing is known
        known_to_be_nontrivial: bool,
    },
    // only executes the interior if the conditional address' value is nonzero
    IfNonZero {
        cond_dp_offset: isize,
        elements: Vec<AST>,
    },
    // Happens with bad code; required to make the loop unrolling sound
    InfiniteLoop,
    // Best-case emission; in theory this could still slip through, but making it explicit
    // makes my life simpler
    AccessOutOfBounds,
    // Adds a given amount to the data pointer. Can be negative to shift left.
    ShiftDataPtr {
        amount: isize,
    },
    // Add the given amount to the byte at the data pointer. Note that due to wrapping,
    // we can (e.g.) subtract 1 by adding 255.
    // dp_offset means "add amount to byte at dp + dp_offset". Used to benefit swapping.
    ModData {
        kind: DatamodKind,
        dp_offset: isize,
    },
    // data[dp + tdo] += data[dp + sdo] * sam
    CombineData {
        source_dp_offset: isize,
        target_dp_offset: isize,
        source_amt_mult: u8,
    },
    // Read a byte from stdin, or whatever IO method is configured
    ReadByte {
        dp_offset: isize,
    },
    // Write a byte to stdout, or whatever IO method is configured
    WriteByte {
        dp_offset: isize,
    },
    WriteConst {
        out: u8,
    },
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub(crate) enum DatamodKind {
    SetData { amount: u8 },
    AddData { amount: u8 },
}

mod optimization;

#[derive(Debug)]
pub enum ParseError {
    // Code point of the illegal end
    EndLoopWithoutStart { code_p: usize },
    // Code point of the started loop that never ended
    UnterminatedLoop { code_p: usize },
}

pub fn full_parse(input_str: &str) -> Result<Vec<CompiledInstr>, ParseError> {
    let mut parsed = parse(input_str)?;
    optimization::optimize(&mut parsed);
    Ok(compile_ast(&parsed))
}

struct ParseStack {
    top_tokens: Vec<AST>,
    running_loops: Vec<(usize, Vec<AST>)>,
}

impl ParseStack {
    fn new() -> Self {
        ParseStack {
            top_tokens: Vec::new(),
            running_loops: Vec::new(),
        }
    }

    fn pop_loop(&mut self) -> Option<(usize, Vec<AST>)> {
        self.running_loops.pop()
    }

    fn start_loop(&mut self, code_p: usize) {
        self.running_loops.push((code_p, Vec::new()));
    }

    fn push_command(&mut self, ast: AST) {
        if self.running_loops.is_empty() {
            self.top_tokens.push(ast);
        } else {
            let last_loop_ind = self.running_loops.len() - 1;
            self.running_loops.get_mut(last_loop_ind).unwrap().1.push(ast);
        }
    }

    fn complete(mut self) -> Result<Vec<AST>, ParseError> {
        if self.running_loops.is_empty() {
            Ok(self.top_tokens)
        } else {
            Err(ParseError::UnterminatedLoop {
                code_p: self.running_loops.pop().unwrap().0,
            })
        }
    }
}

pub(crate) fn parse(data: &str) -> Result<Vec<AST>, ParseError> {
    let mut parse_stack = ParseStack::new();

    for (code_p, token) in lex(&mut data.chars()) {
        match token {
            BfCmd::LoopEnd => {
                if let Some((_, running_loop)) = parse_stack.pop_loop() {
                    let next = AST::Loop {
                        elements: running_loop,
                        cond_dp_offset: 0,
                        known_to_be_nontrivial: false,
                    };
                    parse_stack.push_command(next);
                } else {
                    return Err(ParseError::EndLoopWithoutStart { code_p });
                }
            }
            BfCmd::LoopStart => {
                parse_stack.start_loop(code_p);
            }
            BfCmd::ReadByte => parse_stack.push_command(AST::ReadByte { dp_offset: 0 }),
            BfCmd::WriteByte => parse_stack.push_command(AST::WriteByte { dp_offset: 0 }),
            BfCmd::DecData => parse_stack.push_command(AST::ModData {
                kind: DatamodKind::AddData {
                    amount: 0_u8.wrapping_sub(1),
                },
                dp_offset: 0,
            }),
            BfCmd::IncData => parse_stack.push_command(AST::ModData {
                kind: DatamodKind::AddData { amount: 1 },
                dp_offset: 0,
            }),
            BfCmd::DecPtr => parse_stack.push_command(AST::ShiftDataPtr { amount: -1 }),
            BfCmd::IncPtr => parse_stack.push_command(AST::ShiftDataPtr { amount: 1 }),
        }
    }

    parse_stack.complete()
}

// Lexing BF code is ... astoundingly simple
fn lex<T: Iterator<Item = char>>(iter: &mut T) -> impl Iterator<Item = (usize, BfCmd)> + '_ {
    fn match_char(c: char) -> Option<BfCmd> {
        match c {
            '>' => Some(BfCmd::IncPtr),
            '<' => Some(BfCmd::DecPtr),
            '+' => Some(BfCmd::IncData),
            '-' => Some(BfCmd::DecData),
            '.' => Some(BfCmd::WriteByte),
            ',' => Some(BfCmd::ReadByte),
            '[' => Some(BfCmd::LoopStart),
            ']' => Some(BfCmd::LoopEnd),
            _ => None,
        }
    }

    iter.enumerate()
        .filter_map(|(code_p, text_char)| match_char(text_char).map(|cmd| (code_p, cmd)))
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
enum BfCmd {
    IncPtr,
    DecPtr,
    IncData,
    DecData,
    ReadByte,
    WriteByte,
    LoopStart,
    LoopEnd,
}

pub(crate) fn compile_ast(cmds: &[AST]) -> Vec<CompiledInstr> {
    println!("optimized AST {:#?}", cmds);
    let mut out = Vec::new();

    // Note: we assume brackets are matched, so we don't ever check for it
    // This is just a recursion helper
    compile_ast_helper(&mut out, cmds);

    out
}

fn compile_ast_helper(out: &mut Vec<CompiledInstr>, cmds: &[AST]) {
    for cmd in cmds {
        match cmd {
            AST::Loop {
                elements,
                cond_dp_offset,
                known_to_be_nontrivial: _,
            } => {
                let start_ip = out.len();

                // we need to fix this target_ip but only know what it is after we compile it
                out.push(CompiledInstr::JumpIfZero {
                    target_ip: 0,
                    cond_dp_offset: *cond_dp_offset,
                });

                compile_ast_helper(out, elements);

                out.push(CompiledInstr::JumpIfNonzero {
                    target_ip: start_ip,
                    cond_dp_offset: *cond_dp_offset,
                });

                // Jump to the next instruction after the loop is over
                let end_ip = out.len();

                *out.get_mut(start_ip).unwrap() = CompiledInstr::JumpIfZero {
                    target_ip: end_ip,
                    cond_dp_offset: *cond_dp_offset,
                };
            }
            AST::IfNonZero { elements, cond_dp_offset } => {
                let start_ip = out.len();

                // we need to fix this target_ip but only know what it is after we compile it
                out.push(CompiledInstr::JumpIfZero {
                    target_ip: 0,
                    cond_dp_offset: *cond_dp_offset,
                });

                compile_ast_helper(out, elements);

                // Jump to the next instruction after the branch stuff is over
                let end_ip = out.len();

                *out.get_mut(start_ip).unwrap() = CompiledInstr::JumpIfZero {
                    target_ip: end_ip,
                    cond_dp_offset: *cond_dp_offset,
                };
            }
            AST::InfiniteLoop => {
                out.push(CompiledInstr::InfiniteLoop);
            }
            AST::AccessOutOfBounds => {
                out.push(CompiledInstr::AccessOutOfBounds);
            }
            AST::ShiftDataPtr { amount } => {
                let amount = *amount;
                if amount > 0 {
                    out.push(CompiledInstr::AddPtr { amount: amount as usize });
                } else if amount < 0 {
                    out.push(CompiledInstr::SubPtr {
                        amount: (-amount) as usize,
                    });
                }
            }
            AST::ModData { kind, dp_offset } => {
                out.push(match kind {
                    DatamodKind::AddData { amount } => CompiledInstr::AddData {
                        amount: *amount,
                        dp_offset: *dp_offset,
                    },
                    DatamodKind::SetData { amount } => CompiledInstr::SetData {
                        amount: *amount,
                        dp_offset: *dp_offset,
                    },
                });
            }
            AST::ReadByte { dp_offset } => out.push(CompiledInstr::ReadByte { dp_offset: *dp_offset }),
            AST::WriteByte { dp_offset } => out.push(CompiledInstr::WriteByte { dp_offset: *dp_offset }),
            AST::WriteConst { out: out_byte } => out.push(CompiledInstr::WriteConst { out: *out_byte }),
            AST::CombineData {
                source_dp_offset,
                target_dp_offset,
                source_amt_mult,
            } => {
                out.push(CompiledInstr::AddTwoData {
                    source_dp_offset: *source_dp_offset,
                    target_dp_offset: *target_dp_offset,
                    source_amt_mult: *source_amt_mult,
                });
            }
        }
    }
}
