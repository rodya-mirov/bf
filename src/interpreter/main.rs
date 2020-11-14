use std::env;
use std::error::Error;
use std::fs;

use bf_lib::{parse, Instr};
use io::{Input, Output};

mod io;

struct VM {
    instr: Vec<Instr>,
    ip: usize,

    data: [u8; 30_000],
    dp: usize,
}

impl VM {
    fn new(code: Vec<Instr>) -> Self {
        Self {
            instr: code,
            ip: 0,
            data: [0; 30_000],
            dp: 0,
        }
    }

    fn run<I: Input, O: Output>(
        mut self,
        input: &mut I,
        output: &mut O,
    ) -> Result<(), Box<dyn Error>> {
        while self.ip < self.instr.len() {
            match self.instr.get_mut(self.ip).unwrap() {
                Instr::LoopEnd { start_ip, .. } => {
                    if self.data[self.dp] != 0 {
                        self.ip = *start_ip;
                    } else {
                        self.ip += 1;
                    }
                }
                Instr::LoopStart { end_ip, .. } => {
                    if self.data[self.dp] == 0 {
                        self.ip = *end_ip + 1;
                    } else {
                        self.ip += 1;
                    }
                }
                Instr::IncByte { .. } => {
                    self.data[self.dp] = u8::wrapping_add(self.data[self.dp], 1);
                    self.ip += 1;
                }
                Instr::DecByte { .. } => {
                    self.data[self.dp] = u8::wrapping_sub(self.data[self.dp], 1);
                    self.ip += 1;
                }
                Instr::IncPtr { .. } => {
                    self.dp += 1;
                    self.ip += 1;
                }
                Instr::DecPtr { .. } => {
                    self.dp -= 1;
                    self.ip += 1;
                }
                Instr::ReadByte { .. } => {
                    let read = input.read_byte()?;
                    self.data[self.dp] = read;
                    self.ip += 1;
                }
                Instr::WriteByte { .. } => {
                    let write = self.data[self.dp];
                    output.write_byte(write)?;
                    self.ip += 1;
                }
            }
        }

        Ok(())
    }
}

fn run(input_str: String) -> Result<(), ()> {
    let code = parse(&input_str)?;

    let mut input = io::StdIn::new();
    let mut output = io::StdOut::new();

    let vm = VM::new(code);

    match vm.run(&mut input, &mut output) {
        Err(e) => {
            println!("IO Error: {}", e);
            Err(())
        }
        Ok(_) => Ok(()),
    }
}

fn main() {
    let mut args: Vec<String> = env::args().collect();

    if args.len() != 2 {
        println!("Usage: cargo run bf_interpreter -- infile");
        std::process::exit(1);
    }

    let infile = args.pop().unwrap();
    let input_str = match fs::read_to_string(&infile) {
        Ok(s) => s,
        Err(e) => {
            println!("Error reading file {}: {}", infile, e);
            std::process::exit(1);
        }
    };

    let out = run(input_str);

    if let Err(_) = out {
        std::process::exit(1)
    }
}
