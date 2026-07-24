//! Container log output as the viewer renders it: a bounded tail of lines, each
//! tagged with the stream it came from.
//!
//! The Engine API does not hand back lines. It hands back **frames** — whatever
//! happened to be in the pipe when it flushed — so one frame can carry several
//! lines, and a line can be split across two frames. [`lines_from_frames`] is
//! that reassembly, and it is the reason this is a model rather than three lines
//! inside the service: it is pure, it has real edge cases (a frame ending
//! mid-line, stdout and stderr interleaving, `\r\n` from a Windows image), and it
//! is unit tested without a daemon.
//!
//! Deliberately *not* here, and noted as future work in `docker/mod.rs`: follow
//! mode (a live stream), a filter box, and ANSI colour parsing. The line text is
//! kept exactly as the container wrote it, escape codes and all.

/// Which of a container's two output streams a line came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogStream {
    Stdout,
    Stderr,
}

/// One line of container output.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LogLine {
    pub stream: LogStream,
    /// The line as written, minus its line terminator. Never contains `\n`.
    pub text: String,
}

/// How many lines the viewer asks the engine for, and keeps. Docker Desktop
/// shows a comparable window: enough to see what a container has been doing,
/// bounded so a chatty container cannot turn the panel into an unbounded buffer
/// of elements. The engine applies the same bound with its `tail` parameter, so
/// this is a second belt on top of the braces.
pub const LOG_TAIL_LIMIT: usize = 500;

/// Reassembles engine frames into whole lines, in arrival order.
///
/// A frame that does not end in a newline is a partial line: its remainder is
/// held until the next frame **of the same stream** continues it, so a message
/// split across two flushes is one line, and an interleaved line on the other
/// stream does not get spliced into the middle of it. Anything still pending
/// when the stream ends is emitted as a final, unterminated line.
pub fn lines_from_frames(frames: impl IntoIterator<Item = (LogStream, String)>) -> Vec<LogLine> {
    let mut lines = Vec::new();
    let mut pending_out = String::new();
    let mut pending_err = String::new();

    for (stream, chunk) in frames {
        let pending = match stream {
            LogStream::Stdout => &mut pending_out,
            LogStream::Stderr => &mut pending_err,
        };
        pending.push_str(&chunk);
        // Everything before the final newline is complete; what follows it (which
        // is empty when the frame ended cleanly) stays pending.
        while let Some(index) = pending.find('\n') {
            let line: String = pending.drain(..=index).collect();
            lines.push(LogLine {
                stream,
                text: trim_terminator(&line),
            });
        }
    }

    for (stream, pending) in [
        (LogStream::Stdout, pending_out),
        (LogStream::Stderr, pending_err),
    ] {
        if !pending.is_empty() {
            lines.push(LogLine {
                stream,
                text: trim_terminator(&pending),
            });
        }
    }
    lines
}

/// The last `limit` lines, oldest first. A `limit` of zero keeps nothing.
pub fn tail(mut lines: Vec<LogLine>, limit: usize) -> Vec<LogLine> {
    if lines.len() > limit {
        lines.drain(..lines.len() - limit);
    }
    lines
}

/// Drops a trailing `\n` and the `\r` a CRLF image leaves in front of it.
fn trim_terminator(line: &str) -> String {
    line.trim_end_matches('\n')
        .trim_end_matches('\r')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{LOG_TAIL_LIMIT, LogLine, LogStream, lines_from_frames, tail};

    fn out(text: &str) -> (LogStream, String) {
        (LogStream::Stdout, text.to_string())
    }

    fn err(text: &str) -> (LogStream, String) {
        (LogStream::Stderr, text.to_string())
    }

    fn texts(lines: &[LogLine]) -> Vec<&str> {
        lines.iter().map(|line| line.text.as_str()).collect()
    }

    #[test]
    fn one_frame_can_carry_several_lines() {
        let lines = lines_from_frames([out("first\nsecond\nthird\n")]);
        assert_eq!(texts(&lines), ["first", "second", "third"]);
        assert!(lines.iter().all(|line| line.stream == LogStream::Stdout));
    }

    #[test]
    fn a_line_split_across_frames_is_rejoined() {
        let lines = lines_from_frames([out("hello, "), out("world\n")]);
        assert_eq!(texts(&lines), ["hello, world"]);
    }

    #[test]
    fn an_interleaved_stream_does_not_splice_into_a_partial_line() {
        let lines = lines_from_frames([out("half "), err("boom\n"), out("done\n")]);
        assert_eq!(texts(&lines), ["boom", "half done"]);
        assert_eq!(lines[0].stream, LogStream::Stderr);
        assert_eq!(lines[1].stream, LogStream::Stdout);
    }

    #[test]
    fn an_unterminated_tail_is_still_a_line() {
        let lines = lines_from_frames([out("done\nno newline here")]);
        assert_eq!(texts(&lines), ["done", "no newline here"]);
    }

    #[test]
    fn crlf_terminators_are_trimmed() {
        let lines = lines_from_frames([out("windows\r\nunix\n")]);
        assert_eq!(texts(&lines), ["windows", "unix"]);
    }

    #[test]
    fn a_container_with_no_output_yields_no_lines() {
        assert!(lines_from_frames([]).is_empty());
        assert!(lines_from_frames([out("")]).is_empty());
    }

    #[test]
    fn blank_lines_survive() {
        let lines = lines_from_frames([out("a\n\nb\n")]);
        assert_eq!(texts(&lines), ["a", "", "b"]);
    }

    #[test]
    fn tail_keeps_the_last_lines_oldest_first() {
        let lines = lines_from_frames([out("1\n2\n3\n4\n")]);
        assert_eq!(texts(&tail(lines.clone(), 2)), ["3", "4"]);
        // A tail wider than the log keeps everything.
        assert_eq!(
            texts(&tail(lines.clone(), LOG_TAIL_LIMIT)),
            ["1", "2", "3", "4"]
        );
        assert!(tail(lines, 0).is_empty());
    }
}
