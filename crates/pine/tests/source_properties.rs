//! Properties of the Pine front end over untrusted source: no text makes the
//! lexer, the parser or the compile behind them panic or overflow the stack,
//! every token and error points inside the source, and nesting past the
//! parser's bound is refused with an error instead of recursion.

use proptest::prelude::*;
use quantick_pine::lexer::{Tok, lex};
use quantick_pine::parser::parse;
use quantick_pine::{compile, render_errors, validate};

/// Fragments of real Pine and of near misses, so generated text reaches the
/// parser and the passes instead of stopping at the first lexer error.
const FRAGMENTS: &[&str] = &[
    "//@version=5\n",
    "//@version=6\n",
    "indicator(\"t\")\n",
    "plot(",
    "close",
    "open",
    "x",
    "y",
    " = ",
    " := ",
    " += ",
    " + ",
    " - ",
    " * ",
    " / ",
    " % ",
    " == ",
    " > ",
    "(",
    ")",
    "[",
    "]",
    ", ",
    " ? ",
    " : ",
    " => ",
    "if ",
    "else",
    "for ",
    " to ",
    " by ",
    "while ",
    "switch ",
    "var ",
    "varip ",
    " and ",
    " or ",
    "not ",
    "true",
    "false",
    "na",
    "0",
    "1",
    "1.5",
    "1e309",
    "9223372036854775808",
    "#ff0000",
    "#zz",
    "\"s\"",
    "'s",
    "\n",
    "\n    ",
    "\n        ",
    "\n\t",
    "\n  ",
    " ",
    "ta.sma(",
    "math.max(",
    "array.new_float(",
    "array.get(",
    "input.int(",
    "alertcondition(",
    "strategy.entry(",
    "request.security(",
    "f(a) =>",
    "method ",
    "type ",
    "import ",
    "export ",
    "// c",
    "\\",
    "@",
    "é",
    "€",
];

fn pine_like_source() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(FRAGMENTS), 0..64).prop_map(|parts| parts.concat())
}

/// Lex, parse and compile `source`, checking what each stage promises.
fn exercise(source: &str) {
    match lex(source) {
        Ok(lexed) => {
            let last = lexed.tokens.last().expect("a lexed stream ends in Eof");
            assert_eq!(last.tok, Tok::Eof);
            for token in &lexed.tokens {
                assert!(token.span.start <= token.span.end, "{token:?}");
                assert!(token.span.end <= source.len(), "{token:?}");
            }
            if let Err(errors) = parse(&lexed.tokens) {
                let _ = render_errors(&errors, "p.pine", source);
            }
        }
        Err(errors) => {
            assert!(!errors.is_empty());
            let _ = render_errors(&errors, "p.pine", source);
        }
    }
    if let Err(errors) = compile(source, "t") {
        assert!(!errors.is_empty());
        let _ = render_errors(&errors, "p.pine", source);
    }
    let _ = validate(source, "t");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn arbitrary_text_never_panics_the_front_end(source in "\\PC{0,256}") {
        exercise(&source);
    }

    #[test]
    fn arbitrary_bytes_never_panic_the_front_end(bytes in prop::collection::vec(any::<u8>(), 0..256)) {
        exercise(&String::from_utf8_lossy(&bytes));
    }

    #[test]
    fn pine_like_text_never_panics_the_front_end(source in pine_like_source()) {
        exercise(&source);
    }
}

/// One way a script can nest: `depth` levels of it around a literal.
fn nested(kind: &str, depth: usize) -> String {
    let body = match kind {
        "parens" => format!("{}1{}", "(".repeat(depth), ")".repeat(depth)),
        "calls" => format!("{}1{}", "math.abs(".repeat(depth), ")".repeat(depth)),
        "negation" => format!("{}1", "- ".repeat(depth)),
        "not" => format!("{}true ? 1 : 0", "not ".repeat(depth)),
        "ternary" => format!("{}0", "true ? 1 : ".repeat(depth)),
        "ternary_middle" => format!("{}0{}", "true ? ".repeat(depth), " : 1".repeat(depth)),
        "history" => format!("close{}", "[1]".repeat(depth)),
        "blocks" => {
            let mut source = String::from("//@version=5\nindicator(\"t\")\nx = 0\n");
            for level in 0..depth {
                source.push_str(&" ".repeat(level));
                source.push_str("if true\n");
            }
            source.push_str(&" ".repeat(depth));
            source.push_str("x := 1\nplot(x)\n");
            return source;
        }
        other => panic!("unknown nesting {other}"),
    };
    format!("//@version=5\nindicator(\"t\")\nx = {body}\nplot(x)\n")
}

const NESTINGS: [&str; 8] = [
    "parens",
    "calls",
    "negation",
    "not",
    "ternary",
    "ternary_middle",
    "history",
    "blocks",
];

/// Far past any script a person writes, and deep enough that unbounded
/// recursion would overflow a test thread's stack.
const HOSTILE_DEPTH: usize = 20_000;

/// Hostile depths end in an error naming the nesting, never in recursion
/// deep enough to abort the process.
#[test]
fn nesting_far_past_the_bound_is_an_error_not_a_stack_overflow() {
    for kind in NESTINGS {
        // Blocks cost the square of their depth in indentation.
        let depth = if kind == "blocks" {
            2_000
        } else {
            HOSTILE_DEPTH
        };
        let source = nested(kind, depth);
        let lexed = lex(&source);
        if let Ok(lexed) = &lexed {
            let _ = parse(&lexed.tokens);
        }
        if kind == "history" {
            // A postfix chain is a loop, not a descent: any length parses.
            continue;
        }
        let Err(errors) = compile(&source, "t") else {
            panic!("{kind}: {depth} levels compiled");
        };
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("nests deeper")),
            "{kind}: {}",
            render_errors(&errors, "p.pine", &source)
        );
    }
}

/// Ordinary depths still compile: the bound refuses only hostile input.
#[test]
fn nesting_a_person_writes_still_compiles() {
    for kind in NESTINGS {
        let source = nested(kind, 16);
        if let Err(errors) = compile(&source, "t") {
            panic!("{kind}: {}", render_errors(&errors, "p.pine", &source));
        }
    }
}
