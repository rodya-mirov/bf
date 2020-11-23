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

#[cfg(test)]
mod tests {
    #[derive(Eq, PartialEq, Debug)]
    struct FixedInput {
        dp: usize,
        reads: usize,
        data: Vec<u8>,
    }

    impl FixedInput {
        fn new(data: &str) -> FixedInput {
            FixedInput {
                data: data.as_bytes().to_vec(),
                dp: 0,
                reads: 0,
            }
        }
    }

    impl crate::io::Input for FixedInput {
        type InputError = std::io::Error;

        fn read_byte(&mut self) -> Result<u8, Self::InputError> {
            if self.dp >= self.data.len() {
                return Ok(0); // EOF, by definition
            }

            let out = self.data[self.dp];
            self.dp += 1;
            self.reads += 1;
            Ok(out)
        }
    }

    #[derive(Default, Eq, PartialEq, Debug)]
    struct OutputCapture {
        data: Vec<u8>,
    }

    impl crate::io::Output for OutputCapture {
        type OutputError = std::io::Error;

        fn write_byte(&mut self, byte: u8) -> Result<(), Self::OutputError> {
            self.data.push(byte);
            Ok(())
        }
    }

    fn assert_opt_is_basic(source_str: &str, input_str: &str) {
        let mut opt_input = FixedInput::new(input_str);
        let mut opt_output = OutputCapture::default();

        let opt_code = bf_lib::optimized_parse(source_str).unwrap();

        let opt_result = crate::opt_vm::OptVM::new(opt_code).run(&mut opt_input, &mut opt_output);

        let mut simple_input = FixedInput::new(input_str);
        let mut simple_output = OutputCapture::default();

        let simple_code = bf_lib::simple_parse(source_str).unwrap();

        let simple_result = crate::simple_vm::SimpleVM::new(simple_code).run(&mut simple_input, &mut simple_output);

        // First, assert the "exit status" is the same
        assert_eq!(simple_result.is_ok(), opt_result.is_ok());

        // First, assert they give the same output (even if they errored)
        assert_eq!(simple_output, opt_output);

        // Next, assert they have the same input status (even if they errored)
        assert_eq!(simple_input, opt_input);
    }

    #[test]
    fn test_hello_world() {
        assert_opt_is_basic(include_str!("../../input/hello_world.b"), "");
    }

    #[test]
    fn test_quine() {
        assert_opt_is_basic(include_str!("../../input/quine.b"), "");
    }

    #[test]
    fn test_rot13() {
        assert_opt_is_basic(include_str!("../../input/rot13.b"), "hello");
        assert_opt_is_basic(include_str!("../../input/rot13.b"), "14ewd2fdsfw");
        assert_opt_is_basic(include_str!("../../input/rot13.b"), "fsdfw4f4fwcv");
        assert_opt_is_basic(include_str!("../../input/rot13.b"), "f2rf2wfc!!#2eds\n@de");
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn test_factor() {
        assert_opt_is_basic(include_str!("../../input/factor.b"), "3141343\n");
        assert_opt_is_basic(include_str!("../../input/factor.b"), "123133313\n");
        assert_opt_is_basic(include_str!("../../input/factor.b"), "147\n");
        assert_opt_is_basic(include_str!("../../input/factor.b"), "14747\n");
        assert_opt_is_basic(include_str!("../../input/factor.b"), "1474747\n");
        assert_opt_is_basic(include_str!("../../input/factor.b"), "147474747\n");
        assert_opt_is_basic(include_str!("../../input/factor.b"), "13333333333337\n");
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn test_hanoi() {
        assert_opt_is_basic(include_str!("../../input/hanoi.b"), "");
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn test_mandelbrot() {
        assert_opt_is_basic(include_str!("../../input/mandelbrot.b"), "");
    }
}
