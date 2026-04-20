use rustyline::completion::Completer;
use rustyline::highlight::{CmdKind, Highlighter};
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{Config, Context, Editor, Helper};
use std::borrow::Cow;

struct MyHelper;

impl Completer for MyHelper {
    type Candidate = String;
    fn complete(
        &self,
        _line: &str,
        _pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<String>)> {
        Ok((0, vec![]))
    }
}
impl Hinter for MyHelper {
    type Hint = String;
}
impl Highlighter for MyHelper {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        if line == "/" || line == "/h" {
            // ANSI trick: save cursor, move down, clear below, print menu, restore cursor
            let menu = if line == "/" {
                "  /help\n  /config\n  /exit"
            } else {
                "  /help"
            };
            let s = format!("\x1b[s\x1b[1E\x1b[J{menu}\x1b[u{line}");
            Cow::Owned(s)
        } else {
            // When menu is no longer shown, we should still clear below so it disappears!
            // But only if we previously showed it...
            // Actually, just clearing below on every highlight might clear valid terminal output above?
            // No, \x1b[J clears from cursor down. It clears the lines below the prompt.
            let s = format!("\x1b[s\x1b[1E\x1b[J\x1b[u{line}");
            Cow::Owned(s)
        }
    }
    fn highlight_char(&self, _line: &str, _pos: usize, _kind: CmdKind) -> bool {
        true
    }
}
impl Validator for MyHelper {}
impl Helper for MyHelper {}

fn main() {
    let mut editor = Editor::<MyHelper, _>::with_config(Config::default()).unwrap();
    editor.set_helper(Some(MyHelper));
    let _ = editor.readline("> ");
}
