#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Instr {
    IncPtr { code_p: usize },
    DecPtr { code_p: usize },
    IncByte { code_p: usize },
    DecByte { code_p: usize },
    ReadByte { code_p: usize },
    WriteByte { code_p: usize },
    LoopStart { code_p: usize, end_ip: usize },
    LoopEnd { code_p: usize, start_ip: usize },
}

pub fn parse(input_str: &str) -> Result<Vec<Instr>, ()> {
    use crate::Instr::*;

    let mut code = Vec::new();

    // ip of start counter; used to go back and modify the loop start variable to point to the
    // right place
    let mut loop_stack: Vec<usize> = Vec::new();
    let mut ip = 0;

    for (code_p, code_char) in input_str.chars().enumerate() {
        let next_code = match code_char {
            '>' => Some(IncPtr { code_p }),
            '<' => Some(DecPtr { code_p }),
            '+' => Some(IncByte { code_p }),
            '-' => Some(DecByte { code_p }),
            '.' => Some(WriteByte { code_p }),
            ',' => Some(ReadByte { code_p }),
            '[' => {
                loop_stack.push(ip);
                // end_ip will be modified when we find the end
                Some(LoopStart { code_p, end_ip: 0 })
            }
            ']' => {
                if loop_stack.is_empty() {
                    println!("No loop start for the loop end at codepoint {}", code_p);
                    return Err(());
                }
                let start_ip = loop_stack.pop().unwrap();
                match code.get_mut(start_ip) {
                    None => {
                        println!(
                            "At codepoint {}, loop start pointer {} is invalid; only {} ips so far",
                            code_p,
                            start_ip,
                            code.len()
                        );
                        return Err(());
                    }
                    Some(LoopStart { ref mut end_ip, .. }) => {
                        *end_ip = ip;
                    }
                    Some(other) => {
                        println!("At codepoint {}, loop start pointer {} is pointing to {:?} which is not a loop start", code_p, start_ip, other);
                        return Err(());
                    }
                }
                Some(LoopEnd { code_p, start_ip })
            }
            // skip it, it's a comment
            _ => None,
        };

        if let Some(next_code) = next_code {
            code.push(next_code);
            ip += 1;
        }
    }

    if !loop_stack.is_empty() {
        println!(
            "At end of parsing, {} loops remain unclosed, which is an error.",
            loop_stack.len()
        );
        return Err(());
    }

    Ok(code)
}
