use std::env;
use std::fs;

use bf_lib::{optimized_parse, simple_parse};

mod io;
mod opt_vm;
mod simple_vm;

fn run(input_str: String, is_opt: bool) -> Result<(), ()> {
    let mut input = io::StdIn::new();
    let mut output = io::StdOut::new();

    let handle_parse_error = |e| {
        println!("Parse error: {:#?}", e);
        ()
    };

    let res = if is_opt {
        let code = optimized_parse(&input_str).map_err(handle_parse_error)?;
        println!("Post optimization, executing {} code lines", code.len());
        let vm = opt_vm::OptVM::new(code);

        vm.run(&mut input, &mut output)
    } else {
        let code = simple_parse(&input_str)?;
        println!("Post parse, executing {} code lines", code.len());
        let vm = simple_vm::SimpleVM::new(code);

        vm.run(&mut input, &mut output)
    };

    match res {
        Err(e) => {
            println!("IO Error: {}", e);
            Err(())
        }
        Ok(_) => Ok(()),
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        println!("Usage: cargo run bf_interpreter -- infile [opt]");
        std::process::exit(1);
    }

    let infile = args.get(1).unwrap();

    let is_opt = match args.get(2) {
        None => false,
        Some(_) => true,
    };

    let input_str = match fs::read_to_string(infile) {
        Ok(s) => s,
        Err(e) => {
            println!("Error reading file {}: {}", infile, e);
            std::process::exit(1);
        }
    };

    let out = run(input_str, is_opt);

    if let Err(_) = out {
        std::process::exit(1)
    }
}
