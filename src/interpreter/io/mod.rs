use std::collections::VecDeque;
use std::io::Write;

pub trait Input {
    type InputError: std::error::Error + 'static;

    fn read_byte(&mut self) -> Result<u8, Self::InputError>;
}

pub struct StdIn {
    eof: bool,
    input_buffer: VecDeque<u8>,
}

impl StdIn {
    pub fn new() -> Self {
        Self {
            eof: false,
            input_buffer: VecDeque::new(),
        }
    }
}

impl Input for StdIn {
    type InputError = std::io::Error;

    fn read_byte(&mut self) -> Result<u8, Self::InputError> {
        if self.eof {
            return Ok(0);
        }

        while self.input_buffer.is_empty() {
            let mut to_read = String::new();
            std::io::stdin().read_line(&mut to_read)?;
            if to_read.is_empty() {
                self.eof = true;
                return Ok(0);
            }
            // Tediously, it's impossible to do input from the terminal without adding newlines
            // or bringing in a huge and frustrating dependency (a curses variant)
            for char in to_read.bytes() {
                if char == '\n' as u8 {
                    self.input_buffer.push_back(10); // spec???
                } else {
                    self.input_buffer.push_back(char);
                }
            }
        }

        Ok(self.input_buffer.pop_front().unwrap())
    }
}

pub trait Output {
    type OutputError: std::error::Error + 'static;

    fn write_byte(&mut self, byte: u8) -> Result<(), Self::OutputError>;
}

pub struct StdOut(());

impl StdOut {
    pub fn new() -> Self {
        StdOut(())
    }
}

impl Output for StdOut {
    type OutputError = std::io::Error;

    fn write_byte(&mut self, byte: u8) -> Result<(), Self::OutputError> {
        std::io::stdout().lock().write(&[byte])?;
        Ok(())
    }
}
