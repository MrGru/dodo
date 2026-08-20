//! Type at the Vietnamese engine from a terminal, with no input method
//! installed and no operating system integration at all.
//!
//! ```text
//! cargo run -p dodo-ime-core --example telex                  # interactive
//! cargo run -p dodo-ime-core --example telex -- --keys tieengs # one-shot
//! cargo run -p dodo-ime-core --example telex -- --keys tieengs --verbose
//! ```
//!
//! It is named `telex` because that is the default scheme and the command
//! people will type; `--scheme vni` drives the same engine the other way.
//!
//! # What it shows that the tests cannot
//!
//! The engine never touches text. It answers each keystroke with a list of
//! [`EngineAction`]s and something else performs them — dodo's input listener
//! in production, forty lines of `Host` below here. So the interesting thing to
//! watch is not the final string, it is the split between
//! **committed** text (the document really has it) and the **composition**
//! (provisional, redrawn on every key, gone if the user hits Escape). That
//! distinction is the whole design and it is invisible in an assertion that
//! only compares the finished word. `--verbose` prints the actions themselves.
//!
//! # Why this is line-based and not raw per-keystroke
//!
//! A real key-at-a-time REPL needs the terminal put into raw mode, which on
//! Unix means `termios` through `libc` and on Windows means
//! `SetConsoleMode` — i.e. a new dependency (`crossterm`, `termion`, `libc`)
//! for a developer convenience. The engine is fed one character at a time
//! either way; only the moment the characters arrive differs. So this reads
//! lines, feeds each character to the engine in order, and keeps the
//! composition alive **across** lines — typing `tieen` then `gs` on the next
//! line composes `tiếng` exactly as one burst would. The two things line mode
//! genuinely cannot deliver are a live redraw as you type and a real Backspace
//! key; `:back` covers the second, and the first is what a dependency would
//! have bought.
//!
//! # Not dodo's UI
//!
//! Every string here is a bare literal on purpose. `i18n::Str` governs text a
//! *dodo user* reads; this is a developer tool that is compiled only when
//! someone asks for it (`examples/`, never a `[[bin]]`), it ships in no binary,
//! and it must not link dodo at all — the engine is a crate precisely to keep
//! it independent of gpui. Please do not "fix" these into `Str`.

use dodo_ime_core::core::truncate_graphemes;
use dodo_ime_core::{
    EngineAction, InputScheme, Key, KeyEvent, LanguageEngine, OutputMode, TonePlacement,
    VietnameseConfig, VietnameseEngine,
};
use std::io::{BufRead, Write};

// ---------------------------------------------------------------- the host

/// A document the engine can type into: the production listener's text model,
/// minus the operating system.
///
/// This duplicates `dodo_ime_core::testing::Host` — deliberately, because that
/// one is `#[cfg(test)]` and an example is not a test build, so it is not
/// linkable from here. Forty lines is the cheaper answer to that than making
/// the test helper part of the crate's public surface.
#[derive(Default)]
struct Host {
    /// Text the application has actually accepted.
    document: String,
    /// Provisional text shown but not accepted.
    composition: String,
}

impl Host {
    /// Perform one engine result. `typed` is what the key would have produced,
    /// needed only for `PassThrough` — the host still holds the original event.
    fn apply(&mut self, actions: &[EngineAction], typed: Option<char>) {
        for action in actions {
            match action {
                EngineAction::PassThrough => {
                    if let Some(ch) = typed {
                        self.document.push(ch);
                    }
                }
                EngineAction::InsertText(text) => self.document.push_str(text),
                EngineAction::DeleteBackward(count) => {
                    self.document = truncate_graphemes(&self.document, *count);
                }
                EngineAction::ReplaceBeforeCursor {
                    grapheme_count,
                    text,
                } => {
                    self.document = truncate_graphemes(&self.document, *grapheme_count);
                    self.document.push_str(text);
                }
                EngineAction::SetComposition { text, .. } => self.composition = text.clone(),
                EngineAction::CommitComposition => {
                    self.document.push_str(&self.composition);
                    self.composition.clear();
                }
                EngineAction::ClearComposition => self.composition.clear(),
                EngineAction::ShowCandidates | EngineAction::HideCandidates => {}
            }
        }
    }

    /// Everything on screen: accepted text plus whatever is still provisional.
    fn visible(&self) -> String {
        format!("{}{}", self.document, self.composition)
    }
}

/// One action, short enough to sit on a trace line.
fn describe(action: &EngineAction) -> String {
    match action {
        EngineAction::PassThrough => "PassThrough".to_string(),
        EngineAction::InsertText(text) => format!("InsertText({text:?})"),
        EngineAction::DeleteBackward(count) => format!("DeleteBackward({count})"),
        EngineAction::ReplaceBeforeCursor {
            grapheme_count,
            text,
        } => format!("ReplaceBeforeCursor({grapheme_count} → {text:?})"),
        EngineAction::SetComposition { text, cursor, .. } => {
            format!("SetComposition({text:?}, cursor {cursor})")
        }
        EngineAction::CommitComposition => "CommitComposition".to_string(),
        EngineAction::ClearComposition => "ClearComposition".to_string(),
        EngineAction::ShowCandidates => "ShowCandidates".to_string(),
        EngineAction::HideCandidates => "HideCandidates".to_string(),
    }
}

/// Feed one character and, when asked, say what the engine did with it.
fn feed(engine: &mut VietnameseEngine, host: &mut Host, key: char, verbose: bool) {
    let event = KeyEvent::character(key);
    let result = engine.process_key(&event);
    if verbose {
        let actions = if result.actions.is_empty() {
            "(nothing)".to_string()
        } else {
            result
                .actions
                .iter()
                .map(describe)
                .collect::<Vec<_>>()
                .join(", ")
        };
        let disposition = if result.handled { "handled" } else { "passed " };
        println!("  {key:?}  {disposition}  {actions}");
    }
    host.apply(&result.actions, event.text);
}

// -------------------------------------------------------------- the options

struct Options {
    config: VietnameseConfig,
    keys: Option<String>,
    verbose: bool,
}

const USAGE: &str = "\
dodo-ime-core — drive the Vietnamese engine from a terminal.

  cargo run -p dodo-ime-core --example telex [-- OPTIONS]

Options:
  --keys <sequence>     Type <sequence>, print the result, exit.
  --scheme telex|vni    How diacritics are spelled (default: telex).
  --telex, --vni        Shorthand for --scheme.
  --tones modern|traditional
                        hoà (default) or hòa.
  --output composition|direct
                        Marked text (default) or typed-and-rewritten text.
  --no-spell-check      Keep a non-Vietnamese syllable rendered instead of
                        handing back the keys that were typed.
  -v, --verbose         Print the EngineActions for every keystroke.
  -h, --help            This.

Interactive commands (a line starting with ':'):
  :commit   accept the composition, as leaving the text field would
  :reset    abandon the composition, as clicking elsewhere would
  :back     send one Backspace key to the engine
  :clear    empty the document and the composition
  :scheme telex|vni     switch scheme mid-session
  :verbose  toggle the per-key action trace
  :help     this
  :quit     leave (Ctrl-D also works)";

/// Hand-rolled, because the engine's whole point is that it costs one
/// dependency and a developer example is not where the second one arrives.
fn parse(arguments: Vec<String>) -> Result<Options, String> {
    let mut options = Options {
        config: VietnameseConfig::default(),
        keys: None,
        verbose: false,
    };
    let mut rest = arguments.into_iter();

    while let Some(argument) = rest.next() {
        // `--flag=value` and `--flag value` both, since people type both.
        let (name, inline) = match argument.split_once('=') {
            Some((name, value)) => (name.to_string(), Some(value.to_string())),
            None => (argument, None),
        };
        let mut value = |name: &str| -> Result<String, String> {
            inline
                .clone()
                .or_else(|| rest.next())
                .ok_or_else(|| format!("{name} needs a value"))
        };

        match name.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            "-v" | "--verbose" => options.verbose = true,
            "--keys" => options.keys = Some(value("--keys")?),
            "--telex" => options.config.scheme = InputScheme::Telex,
            "--vni" => options.config.scheme = InputScheme::Vni,
            "--scheme" => options.config.scheme = scheme(&value("--scheme")?)?,
            "--tones" => {
                options.config.tone_placement = match value("--tones")?.as_str() {
                    "modern" => TonePlacement::Modern,
                    "traditional" => TonePlacement::Traditional,
                    other => return Err(format!("unknown tone placement: {other}")),
                }
            }
            "--output" => {
                options.config.output = match value("--output")?.as_str() {
                    "composition" => OutputMode::Composition,
                    "direct" => OutputMode::Direct,
                    other => return Err(format!("unknown output mode: {other}")),
                }
            }
            "--no-spell-check" => options.config.spell_check = false,
            other => return Err(format!("unknown option: {other}")),
        }
    }

    Ok(options)
}

fn scheme(name: &str) -> Result<InputScheme, String> {
    match name {
        "telex" => Ok(InputScheme::Telex),
        "vni" => Ok(InputScheme::Vni),
        other => Err(format!("unknown scheme: {other} (telex or vni)")),
    }
}

// ----------------------------------------------------------------- the modes

/// `--keys tieengs` → `tiếng`. One line out, so it can be piped or pasted into
/// a bug report; `--verbose` puts the trace above it.
fn one_shot(options: &Options, keys: &str) {
    let mut engine = VietnameseEngine::new(options.config);
    let mut host = Host::default();
    for key in keys.chars() {
        feed(&mut engine, &mut host, key, options.verbose);
    }
    // Committing at the end is what a host does when focus leaves the field,
    // and it is why a sequence stopping mid-syllable still yields text.
    let result = engine.commit();
    if options.verbose {
        for action in &result.actions {
            println!("  commit  handled  {}", describe(action));
        }
    }
    host.apply(&result.actions, None);
    println!("{}", host.visible());
}

/// The document and the composition, drawn apart — the point of the exercise.
fn show(host: &Host) {
    let composing = if host.composition.is_empty() {
        "—".to_string()
    } else {
        format!("⟦{}⟧", host.composition)
    };
    println!("  committed  {}", host.document);
    println!("  composing  {composing}");
    println!("  screen     {}", host.visible());
}

fn interactive(options: &mut Options) {
    let mut engine = VietnameseEngine::new(options.config);
    let mut host = Host::default();

    println!(
        "dodo-ime-core: scheme {}, {} output.\n\
         Type keys and press Enter. :help for commands, :quit to leave.\n",
        options.config.scheme.code(),
        match options.config.output {
            OutputMode::Composition => "marked-text",
            OutputMode::Direct => "direct",
        }
    );

    let stdin = std::io::stdin();
    loop {
        print!("> ");
        // The prompt is not newline-terminated, so it has to be pushed out.
        let _ = std::io::stdout().flush();

        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            // Ctrl-D, with the prompt still unterminated on the line.
            Ok(0) => {
                println!();
                break;
            }
            Ok(_) => {}
            Err(error) => {
                eprintln!("stdin: {error}");
                break;
            }
        }
        let line = line.trim_end_matches(['\n', '\r']);

        if let Some(command) = line.strip_prefix(':') {
            let mut words = command.split_whitespace();
            match words.next().unwrap_or("") {
                "quit" | "q" => break,
                "help" | "h" => println!("{USAGE}"),
                "commit" => {
                    let result = engine.commit();
                    host.apply(&result.actions, None);
                }
                "reset" => {
                    let result = engine.reset();
                    host.apply(&result.actions, None);
                }
                "back" => {
                    let event = KeyEvent::special(Key::Backspace);
                    let result = engine.process_key(&event);
                    if options.verbose {
                        for action in &result.actions {
                            println!("  Backspace  {}", describe(action));
                        }
                    }
                    host.apply(&result.actions, event.text);
                }
                "clear" => {
                    let _ = engine.reset();
                    host = Host::default();
                }
                "verbose" => {
                    options.verbose = !options.verbose;
                    println!("  verbose {}", if options.verbose { "on" } else { "off" });
                }
                "scheme" => match words.next().map(scheme) {
                    Some(Ok(chosen)) => {
                        options.config.scheme = chosen;
                        // `set_config` returns the actions that abandon whatever
                        // was in flight under the old scheme; performing them is
                        // what keeps the host's idea of the screen honest.
                        let result = engine.set_config(options.config);
                        host.apply(&result.actions, None);
                        println!("  scheme {}", chosen.code());
                    }
                    Some(Err(error)) => println!("  {error}"),
                    None => println!("  :scheme telex|vni"),
                },
                other => println!("  unknown command: :{other} (try :help)"),
            }
            show(&host);
            continue;
        }

        for key in line.chars() {
            feed(&mut engine, &mut host, key, options.verbose);
        }
        show(&host);
    }
}

fn main() {
    let mut options = match parse(std::env::args().skip(1).collect()) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{error}\n\n{USAGE}");
            std::process::exit(2);
        }
    };

    match options.keys.clone() {
        Some(keys) => one_shot(&options, &keys),
        None => interactive(&mut options),
    }
}
