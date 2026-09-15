use vir::buffer::TextBuffer;
use vir::ex::address::{AddressError, evaluate};
use vir::ex::command::execute;
use vir::ex::parser::{Address, BaseAddress, Command, Range, parse};

#[test]
fn original_command_names_and_number_format() {
    // ex_cmds.c: case 'n', numberit; ex_put.c: numbline.
    let buffer = TextBuffer::from_text("one\ntwo\nthree\n").unwrap();
    for name in ["nu", "num", "number", "#"] {
        let mut current = Some(0);
        let result = execute(parse(&format!("2,3{name}")).unwrap(), &buffer, &mut current).unwrap();
        assert_eq!(result.output, "     2  two\n     3  three\n");
        assert_eq!(current, Some(2));
    }
    for name in ["n", "next"] {
        assert_eq!(parse(name).unwrap().command, Command::Next);
        let mut current = Some(0);
        assert!(
            execute(parse(name).unwrap(), &buffer, &mut current)
                .unwrap_err()
                .contains("not implemented")
        );
        assert_eq!(current, Some(0));
    }
}

#[test]
fn print_updates_dot_and_errors_preserve_it() {
    // ex_cmdsub.c: plines(..., movedot).
    let buffer = TextBuffer::from_text("one\ntwo\nthree\n").unwrap();
    let mut current = Some(0);
    execute(parse("1,2print").unwrap(), &buffer, &mut current).unwrap();
    assert_eq!(current, Some(1));
    assert_eq!(
        execute(parse(".p").unwrap(), &buffer, &mut current)
            .unwrap()
            .output,
        "two\n"
    );
    for input in ["4p", "3,1p", "0p", ".-3p", "1q", "%quit!"] {
        assert!(
            execute(parse(input).unwrap(), &buffer, &mut current).is_err(),
            "{input}"
        );
        assert_eq!(current, Some(1));
    }
    for input in ["q", "q!", "quit", "quit!"] {
        assert!(
            execute(parse(input).unwrap(), &buffer, &mut current)
                .unwrap()
                .quit
        );
    }
}

#[test]
fn whitespace_relative_addresses_and_default_range_endpoints() {
    // ex_addr.c: address; ex_cmds.c: omitted comma endpoints default to dot.
    for (input, start, end) in [
        (" 1 , 3 p ", 0, 2),
        (",3p", 1, 2),
        ("1,p", 0, 1),
        (",p", 1, 1),
        ("+p", 2, 2),
        ("-p", 0, 0),
        ("0+1p", 0, 0),
        ("4-1p", 2, 2),
        (". + 1 p", 2, 2),
    ] {
        let range = evaluate(parse(input).unwrap().range, 3, Some(1)).unwrap();
        assert_eq!((range.start, range.end), (start, end), "{input}");
    }
    for input in ["%,2p", "1,%p", "%+1p", "1,2,3p", "1;p", "p extra"] {
        assert!(parse(input).is_err(), "{input}");
    }
}

#[test]
fn extreme_addresses_and_invalid_state_return_errors() {
    for input in [
        format!("{}0p", usize::MAX),
        format!("1+{}0p", isize::MAX),
        format!("1-{}0p", isize::MAX),
    ] {
        assert!(parse(&input).is_err());
    }
    for input in [format!("{}p", usize::MAX), format!("$+{}p", isize::MAX)] {
        assert!(evaluate(parse(&input).unwrap().range, 3, Some(0)).is_err());
    }
    let range = Range {
        start: Some(Address::Relative {
            base: BaseAddress::Line(usize::MAX),
            offset: 1,
        }),
        end: None,
    };
    assert_eq!(evaluate(range, 3, Some(0)), Err(AddressError::OutOfBounds));
    let range = Range {
        start: Some(Address::Relative {
            base: BaseAddress::Line(1),
            offset: isize::MIN,
        }),
        end: None,
    };
    assert_eq!(evaluate(range, 3, Some(0)), Err(AddressError::OutOfBounds));
    for input in ["p", ".p", "%p", "1p"] {
        for current in [None, Some(3), Some(usize::MAX)] {
            assert!(evaluate(parse(input).unwrap().range, 3, current).is_err());
        }
        assert_eq!(
            evaluate(parse(input).unwrap().range, 0, None),
            Err(AddressError::EmptyBuffer)
        );
    }
}
