//! The blindgate binary is a thin shell over the parse entry.
//!
//! It takes one trace file and one repo root as its arguments
//! and prints the read set the trace holds, one path per line,
//! then the outside reads under their own heading when the run
//! named any. A trace the shape reading refuses is reported in
//! the words of the refusal and exits nonzero. The full report
//! the Python command line renders stays with the Python in
//! this slice, as the contract port section states.

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: blindgate TRACE_FILE ROOT");
        return ExitCode::from(2);
    }
    let text = match std::fs::read_to_string(&args[1]) {
        Ok(text) => text,
        Err(exc) => {
            eprintln!("cannot read trace log {} ({exc})", args[1]);
            return ExitCode::from(2);
        }
    };
    match blindgate::parse_trace(&text, Path::new(&args[2])) {
        Ok(read) => {
            for item in &read.reads {
                println!("{item}");
            }
            if !read.outside_reads.is_empty() {
                println!("outside reads");
                for item in &read.outside_reads {
                    println!("{item}");
                }
            }
            ExitCode::SUCCESS
        }
        Err(exc) => {
            eprintln!("{exc}");
            ExitCode::from(2)
        }
    }
}
