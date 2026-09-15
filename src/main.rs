use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.get(1).map(String::as_str) == Some("--ex") {
        if args.len() > 3 {
            eprintln!("Usage: {} --ex [file]", args[0]);
            std::process::exit(1);
        }
        let mut editor = vir::editor::Editor::default();
        if let Some(path) = args.get(2)
            && let Err(error) = editor.load(Path::new(path))
        {
            eprintln!("Unable to load {path}: {error}");
            std::process::exit(1);
        }
        if let Err(error) = vir::ex::repl::run(
            &mut io::stdin().lock(),
            &mut io::stdout().lock(),
            &mut editor,
        ) {
            eprintln!("ex: {error}");
            std::process::exit(1);
        }
        return;
    }
    if args.len() != 3 {
        eprintln!("Usage: {} <unused-opt> <filename>", args[0]);
        std::process::exit(1);
    }
    let _opt = &args[1];
    let file_path = &args[2];

    let f = File::open(file_path).unwrap_or_else(|error| {
        eprintln!("Unable to Open File {}: {}", file_path, error);
        std::process::exit(1);
    });

    let reader = BufReader::new(f);
    for line in reader.lines() {
        match line {
            Ok(line) => println!("{line}"),
            Err(error) => {
                eprintln!("Unable to read file {file_path}: {error}");
                std::process::exit(1);
            }
        }
    }
}
