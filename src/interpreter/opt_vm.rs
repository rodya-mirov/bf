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
        while self.ip < self.instr.len() {
            match self.instr.get(self.ip).unwrap() {
                CompiledInstr::JumpIfNonzero { target_ip } => {
                    if self.data[self.dp] != 0 {
                        self.ip = *target_ip;
                    } else {
                        self.ip += 1;
                    }
                }
                CompiledInstr::JumpIfZero { target_ip } => {
                    if self.data[self.dp] == 0 {
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
                CompiledInstr::AddPtr { amount } => {
                    self.dp += amount;
                    self.ip += 1;
                }
                CompiledInstr::SubPtr { amount } => {
                    self.dp -= amount;
                    self.ip += 1;
                }
                CompiledInstr::ReadByte { .. } => {
                    let read = input.read_byte()?;
                    self.data[self.dp] = read;
                    self.ip += 1;
                }
                CompiledInstr::WriteByte { .. } => {
                    let write = self.data[self.dp];
                    output.write_byte(write)?;
                    self.ip += 1;
                }
            }
        }

        Ok(())
    }
}
