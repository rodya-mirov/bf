use std::error::Error;

use bf_lib::BfInstr;

use crate::io::{Input, Output};

pub(crate) struct SimpleVM {
    instr: Vec<BfInstr>,
    ip: usize,

    data: [u8; 30_000],
    dp: usize,
}

impl SimpleVM {
    pub(crate) fn new(code: Vec<BfInstr>) -> Self {
        Self {
            instr: code,
            ip: 0,
            data: [0; 30_000],
            dp: 0,
        }
    }

    pub(crate) fn run<I: Input, O: Output>(mut self, input: &mut I, output: &mut O) -> Result<(), Box<dyn Error>> {
        let mut total_instructions = 0;
        while self.ip < self.instr.len() {
            total_instructions += 1;
            match self.instr.get_mut(self.ip).unwrap() {
                BfInstr::LoopEnd { start_ip, .. } => {
                    if self.data[self.dp] != 0 {
                        self.ip = *start_ip;
                    } else {
                        self.ip += 1;
                    }
                }
                BfInstr::LoopStart { end_ip, .. } => {
                    if self.data[self.dp] == 0 {
                        self.ip = *end_ip + 1;
                    } else {
                        self.ip += 1;
                    }
                }
                BfInstr::IncByte { .. } => {
                    self.data[self.dp] = u8::wrapping_add(self.data[self.dp], 1);
                    self.ip += 1;
                }
                BfInstr::DecByte { .. } => {
                    self.data[self.dp] = u8::wrapping_sub(self.data[self.dp], 1);
                    self.ip += 1;
                }
                BfInstr::IncPtr { .. } => {
                    self.dp += 1;
                    self.ip += 1;
                }
                BfInstr::DecPtr { .. } => {
                    self.dp -= 1;
                    self.ip += 1;
                }
                BfInstr::ReadByte { .. } => {
                    let read = input.read_byte()?;
                    self.data[self.dp] = read;
                    self.ip += 1;
                }
                BfInstr::WriteByte { .. } => {
                    let write = self.data[self.dp];
                    output.write_byte(write)?;
                    self.ip += 1;
                }
            }
        }

        println!("Executing took {} instructions", total_instructions);

        Ok(())
    }
}
