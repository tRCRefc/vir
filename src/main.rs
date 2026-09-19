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
    if args.get(1).map(String::as_str) == Some("--recover-list") {
        let result = (|| -> io::Result<()> {
            if args.len() > 3 {
                return Err(io::Error::other("Usage: vir --recover-list [directory]"));
            }
            let directory = args
                .get(2)
                .map(PathBuf::from)
                .or_else(|| env::var_os("VIR_RECOVERY_DIR").map(PathBuf::from))
                .unwrap_or(env::current_dir()?);
            for path in vir::recovery::list(&directory)? {
                match vir::recovery::read(&path) {
                    Ok(snapshot) => println!(
                        "{:?} source={:?} bytes={}",
                        path,
                        snapshot.source,
                        snapshot.buffer.to_text().len()
                    ),
                    Err(error) => println!("{:?} invalid: {error}", path),
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("recovery: {error}");
            std::process::exit(1);
        }
        return;
    }
    if let Some(mode @ ("--vi" | "--ex")) = args.get(1).map(String::as_str) {
        if let Err(error) = run_editor(mode, &args[2..]) {
            eprintln!("{mode}: {error}");
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

fn run_editor(mode: &str, args: &[String]) -> io::Result<()> {
    let mut editor = vir::editor::Editor::default();
    if args.first().map(String::as_str) == Some("--recover") {
        if args.len() != 2 {
            return Err(io::Error::other("Usage: vir --vi|--ex --recover SNAPSHOT"));
        }
        let snapshot = vir::recovery::read(std::path::Path::new(&args[1]))?;
        eprintln!(
            "Recovered text from {:?} into an unnamed buffer; write to a new path. Snapshot retained.",
            snapshot.source
        );
        editor = vir::editor::Editor::from_recovery(snapshot.buffer);
    } else if mode == "--vi" {
        if args.len() > 1 {
            return Err(io::Error::other("Usage: vir --vi [file]"));
        }
        if let Some(path) = args.first() {
            editor.load(std::path::Path::new(path))?;
        }
    } else {
        editor.open_arguments(args.iter().map(PathBuf::from).collect())?;
    }
    if mode == "--vi" {
        vir::visual::run(&mut editor)
    } else {
        let mut recovery = vir::recovery::Recovery::for_editor(&editor)?;
        let prompt = io::stdin().is_terminal() && io::stdout().is_terminal();
        vir::ex::repl::run_with_recovery(
            &mut io::stdin().lock(),
            &mut io::stdout().lock(),
            &mut editor,
            prompt,
            &mut recovery,
        )?;
        recovery.finish()
    }
}
