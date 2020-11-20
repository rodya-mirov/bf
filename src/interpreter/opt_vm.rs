use std::error::Error;

use bf_lib::CompiledInstr;

use crate::io::{Input, Output};

pub(crate) struct OptVM {
    instr: Vec<CompiledInstr>,
    ip: usize,

    data: [u8; 30_000],
    dp: usize,
}

impl OptVM {
    pub(crate) fn new(code: Vec<CompiledInstr>) -> Self {
        Self {
            instr: code,
            ip: 0,
            data: [0; 30_000],
            dp: 0,
        }
    }

    pub(crate) fn run<I: Input, O: Output>(mut self, input: &mut I, output: &mut O) -> Result<(), Box<dyn Error>> {
        let mut total_instructions: u64 = 0;
        while self.ip < self.instr.len() {
            total_instructions += 1;
            match self.instr.get(self.ip).unwrap() {
                CompiledInstr::JumpIfNonzero { target_ip, cond_dp_offset } => {
                    let actual_dp = (self.dp as isize + cond_dp_offset) as usize;
                    if self.data[actual_dp] != 0 {
                        self.ip = *target_ip;
                    } else {
                        self.ip += 1;
                    }
                }
                CompiledInstr::JumpIfZero { target_ip, cond_dp_offset } => {
                    let actual_dp = (self.dp as isize + cond_dp_offset) as usize;
                    if self.data[actual_dp] == 0 {
                        self.ip = *target_ip;
                    } else {
                        self.ip += 1;
                    }
                }
                CompiledInstr::AddData { amount, dp_offset } => {
                    let local_dp = (self.dp as isize + dp_offset) as usize;
                    self.data[local_dp] = u8::wrapping_add(self.data[local_dp], *amount);
                    self.ip += 1;
                }
                CompiledInstr::SetData { amount, dp_offset } => {
                    let local_dp = (self.dp as isize + dp_offset) as usize;
                    self.data[local_dp] = *amount;
                    self.ip += 1;
                }
                CompiledInstr::AddTwoData {
                    source_dp_offset,
                    target_dp_offset,
                    source_amt_mult,
                } => {
                    let source_dp = (self.dp as isize + source_dp_offset) as usize;
                    let target_dp = (self.dp as isize + target_dp_offset) as usize;

                    let addend = u8::wrapping_mul(self.data[source_dp], *source_amt_mult);

                    // Note this addend business is essential -- it's possible target_dp is an
                    // illegal address and the code is "fine" because the interior of the loop is
                    // never executed
                    // TODO: i don't like encoding this magic knowledge in the VM, it should be in the compiled instructions
                    if addend != 0 {
                        self.data[target_dp] = u8::wrapping_add(self.data[target_dp], addend);
                    }
                    self.ip += 1;
                }
                CompiledInstr::AddPtr { amount } => {
                    self.dp += amount;
                    self.ip += 1;
                }
                CompiledInstr::SubPtr { amount } => {
                    self.dp -= amount;
                    self.ip += 1;
                }
                CompiledInstr::ReadByte { dp_offset } => {
                    let actual_dp = (self.dp as isize + dp_offset) as usize;
                    let read = input.read_byte()?;
                    self.data[actual_dp] = read;
                    self.ip += 1;
                }
                CompiledInstr::WriteByte { dp_offset } => {
                    let actual_dp = (self.dp as isize + dp_offset) as usize;
                    let write = self.data[actual_dp];
                    output.write_byte(write)?;
                    self.ip += 1;
                }
            }
        }

        println!("Process took {} instructions", total_instructions);

        Ok(())
    }
}
