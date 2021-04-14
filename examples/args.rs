use std::{env::args, io::stdout};

use fz::select;

fn main() {
    let args: Vec<String> = args().skip(1).collect();
    let args_ref: Vec<&str> = args.iter().map(|a| a.as_str()).collect();

    // select items from args
    for selection in select(stdout(), &args_ref).unwrap().as_ref() {
        println!("{}", selection);
    }
}
