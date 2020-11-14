default:
    @ just --list --unsorted

# initial setup command

build-env:
    docker build -f Dockerfile -t brainfuck-compilers-env .


# commands to run code


interpret name:
    cargo run --release --bin bf_interpreter -- input/{{name}}.b

alias carx := compile-and-run-x86_64-example
alias cara := compile-and-run-aarch64-example
alias carw := compile-and-run-wasm32-wasi-example
alias carl := compile-and-run-llvm-ir-example
alias carc := compile-and-run-c-example

alias carbx := compile-and-run-bf-to-x86_64
alias carba := compile-and-run-bf-to-aarch64
alias carbw := compile-and-run-bf-to-wasm32-wasi
alias carbl := compile-and-run-bf-to-llvm-ir


# commands to compile code & run benchmarks


alias cbat := compile-bf-to-all-targets

build:
    cargo build --release

compile-bf-to-all-targets name:
    cargo build --release --bin bf_interpreter
    cargo run --release --bin bf_to_x86_64_compiler -- ./input/{{name}}.b ./output/x86_64/{{name}}.s
    cargo run --release --bin bf_to_aarch64_compiler -- ./input/{{name}}.b ./output/aarch64/{{name}}.s
    cargo run --release --bin bf_to_wasm32_wasi_compiler -- ./input/{{name}}.b ./output/wasm32_wasi/{{name}}.wat
    cargo run --release --bin bf_to_llvm_ir_compiler -- ./input/{{name}}.b ./output/llvm_ir/{{name}}.ll
    docker run -v $(pwd):/root/project -w /root/project/output brainfuck-compilers-env bash -c "clang -O3 -nostdlib -fno-integrated-as -Wa,-msyntax=intel,-mnaked-reg -s ./x86_64/{{name}}.s -o ./x86_64/{{name}}.out; clang -O3 -nostdlib -fno-integrated-as -target aarch64-linux-gnu -s ./aarch64/{{name}}.s -o ./aarch64/{{name}}.out; wat2wasm ./wasm32_wasi/{{name}}.wat -o ./wasm32_wasi/{{name}}.wasm; clang -O3 -Wno-override-module ./llvm_ir/{{name}}.ll -o ./llvm_ir/{{name}}.out"

benchmark name: build
    time ./target/release/bf_interpreter ./input/{{name}}.b
    docker run -v $(pwd):/root/project -w /root/project/output brainfuck-compilers-env bash -c "time ./x86_64/{{name}}.out; time ./aarch64/{{name}}.out; time PATH=/root/.wasmtime/bin wasmtime ./wasm32_wasi/{{name}}.wasm; time ./llvm_ir/{{name}}.out"


# commands to compile & run examples


compile-and-run-x86_64-example name:
    docker run -it -v $(pwd):/root/project -w /root/project/examples/x86_64 brainfuck-compilers-env bash -c "clang -nostdlib -fno-integrated-as -Wa,-msyntax=intel,-mnaked-reg -s {{name}}.s -o {{name}}.out && ./{{name}}.out; echo -e \"\nExit code: \$?\""

compile-and-run-aarch64-example name:
    docker run -it -v $(pwd):/root/project -w /root/project/examples/aarch64 brainfuck-compilers-env bash -c "clang -nostdlib -fno-integrated-as -target aarch64-linux-gnu -s {{name}}.s -o {{name}}.out && ./{{name}}.out; echo -e \"\nExit code: \$?\""

compile-and-run-wasm32-wasi-example name:
    docker run -it -v $(pwd):/root/project -w /root/project/examples/wasm32_wasi brainfuck-compilers-env bash -c "wat2wasm {{name}}.wat -o {{name}}.wasm; PATH=/root/.wasmtime/bin wasmtime {{name}}.wasm; echo -e \"\nExit code: \$?\""

compile-and-run-llvm-ir-example name:
    docker run -it -v $(pwd):/root/project -w /root/project/examples/llvm_ir brainfuck-compilers-env bash -c "clang -Wno-override-module {{name}}.ll -o {{name}}.out && ./{{name}}.out; echo -e \"\nExit code: \$?\""

compile-and-run-c-example name:
    docker run -it -v $(pwd):/root/project -w /root/project/examples/c brainfuck-compilers-env bash -c "clang {{name}}.c -o {{name}}.out && ./{{name}}.out; echo -e \"\nExit code: \$?\""


# commands to compile & run bf programs


compile-and-run-bf-to-x86_64 name:
    cargo run --release --bin bf_to_x86_64_compiler -- ./input/{{name}}.b ./output/x86_64/{{name}}.s
    docker run -it -v $(pwd):/root/project -w /root/project/output/x86_64 brainfuck-compilers-env bash -c "clang -nostdlib -fno-integrated-as -Wa,-msyntax=intel,-mnaked-reg -s {{name}}.s -o {{name}}.out && ./{{name}}.out"

compile-and-run-bf-to-aarch64 name:
    cargo run --release --bin bf_to_aarch64_compiler -- ./input/{{name}}.b ./output/aarch64/{{name}}.s
    docker run -it -v $(pwd):/root/project -w /root/project/output/aarch64 brainfuck-compilers-env bash -c "clang -nostdlib -fno-integrated-as -target aarch64-linux-gnu -s {{name}}.s -o {{name}}.out && ./{{name}}.out"

compile-and-run-bf-to-wasm32-wasi name:
    cargo run --release --bin bf_to_wasm32_wasi_compiler -- ./input/{{name}}.b ./output/wasm32_wasi/{{name}}.wat
    docker run -it -v $(pwd):/root/project -w /root/project/output/wasm32_wasi brainfuck-compilers-env bash -c "wat2wasm {{name}}.wat -o {{name}}.wasm; PATH=/root/.wasmtime/bin wasmtime {{name}}.wasm"

compile-and-run-bf-to-llvm-ir name:
    cargo run --release --bin bf_to_llvm_ir_compiler -- ./input/{{name}}.b ./output/llvm_ir/{{name}}.ll
    docker run -it -v $(pwd):/root/project -w /root/project/output/llvm_ir brainfuck-compilers-env bash -c "clang -O3 -Wno-override-module {{name}}.ll -o {{name}}.out && ./{{name}}.out"


# commands to build binaries


build-interpreter:
    cargo build --release --bin bf_interpreter

build-x86_64-compiler:
    cargo build --release --bin bf_to_x86_64_compiler

build-aarch64-compiler:
    cargo build --release --bin bf_to_aarch64_compiler

build-wasm32-wasi-compiler:
    cargo build --release --bin bf_to_wasm32_wasi_compiler

build-llvm-ir-compiler:
    cargo build --release --bin bf_to_llvm_ir_compiler


# misc


test:
    cargo test

sh-env:
    docker run -v $(pwd):/root/project -w /root/project -it brainfuck-compilers-env bash