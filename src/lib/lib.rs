mod optimized;
mod simple;

pub use optimized::{full_parse as optimized_parse, CompiledInstr};
pub use simple::{parse as simple_parse, BfInstr};
