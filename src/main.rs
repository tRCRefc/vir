use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, IsTerminal};
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() == 2 && matches!(args[1].as_str(), "--help" | "-h") {
        print!("{}", include_str!("help.txt"));
        return;
    }
    if args.get(1).map(String::as_str) == Some("--ex") {
        let mut editor = vir::editor::Editor::default();
        let paths = args.iter().skip(2).map(PathBuf::from).collect();
        if let Err(error) = editor.open_arguments(paths) {
            eprintln!("Unable to load file: {error}");
            std::process::exit(1);
        }
        let prompt = io::stdin().is_terminal() && io::stdout().is_terminal();
        if let Err(error) = vir::ex::repl::run_with_prompt(
            &mut io::stdin().lock(),
            &mut io::stdout().lock(),
            &mut editor,
            prompt,
        ) {
            eprintln!("ex: {error}");
            std::process::exit(1);
        }
        return;
    }
    if args.len() != 3 {
        eprintln!(
            "Usage: {} --ex [file ...] (legacy: <unused-opt> <filename>)",
            args[0]
        );
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
