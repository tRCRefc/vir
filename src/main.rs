use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: {} -opt -filename", &args[0]);
        return;
    }
    let opt = &args[1];
    let file_path = &args[2];

    let f = File::open(file_path).unwrap_or_else(|error| {
        eprintln!("Unable to Open File {}: {}", file_path, error);
        std::process::exit(1);
    });

    let reader = BufReader::new(f);
    for line in reader.lines() {
        println!("{}", line.unwrap());
    }
}
