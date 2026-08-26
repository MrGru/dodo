//! The template set the editor's floating button inserts, and the one rule for
//! putting one into a buffer that already has something in it.
//!
//! **No GPUI here**, same rule as [`crate::render`], [`crate::workspace`] and
//! [`crate::zoom`]: a template is a `&'static str` and appending is string
//! arithmetic, so both are `#[test]`-able without a window — which is the only
//! kind of test this crate can have (see [`crate::view`]'s module doc).
//!
//! Small and deliberately not extensible from the UI — the workspace plan's
//! own words are "discoverability, not a giant template marketplace" — so this
//! is a plain enum over a `const` example each, not a registry.
//!
//! There is no `Blank` template. There used to be, back when the set hung off
//! the tab bar's "+" button and "new tab" and "new tab from a template" were
//! the same gesture; the "+" button now makes a blank tab directly and this
//! menu appends into the tab you are already in, so a menu entry that appends
//! nothing would be a row that does nothing at all.

use crate::i18n::mermaid;

/// One starting point for a diagram.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MermaidTemplate {
    Flowchart,
    Sequence,
    Class,
    State,
    Er,
    Architecture,
}

impl MermaidTemplate {
    pub(crate) const ALL: [MermaidTemplate; 6] = [
        MermaidTemplate::Flowchart,
        MermaidTemplate::Sequence,
        MermaidTemplate::Class,
        MermaidTemplate::State,
        MermaidTemplate::Er,
        MermaidTemplate::Architecture,
    ];

    pub(crate) fn label(self) -> mermaid::Text {
        match self {
            MermaidTemplate::Flowchart => mermaid::Text::TemplateFlowchart,
            MermaidTemplate::Sequence => mermaid::Text::TemplateSequence,
            MermaidTemplate::Class => mermaid::Text::TemplateClass,
            MermaidTemplate::State => mermaid::Text::TemplateState,
            MermaidTemplate::Er => mermaid::Text::TemplateEr,
            MermaidTemplate::Architecture => mermaid::Text::TemplateArchitecture,
        }
    }

    /// The example source the template inserts. Each was checked against the
    /// real renderer during development (`mermaid-rs-renderer` 0.3.1) —
    /// inserting a template that immediately shows a syntax error would be
    /// worse than no templates at all — and the `every_template_renders`
    /// test below is what keeps that true rather than remembered.
    pub(crate) fn source(self) -> &'static str {
        match self {
            MermaidTemplate::Flowchart => {
                "flowchart LR\n  A[Start] --> B{Decision}\n  B -->|Yes| C[Do it]\n  B -->|No| D[Skip]\n"
            }
            MermaidTemplate::Sequence => {
                "sequenceDiagram\n  participant Alice\n  participant Bob\n  Alice->>Bob: Hello Bob\n  Bob-->>Alice: Hi Alice\n"
            }
            MermaidTemplate::Class => {
                "classDiagram\n  Animal <|-- Duck\n  Animal : +String name\n  Animal : +makeSound()\n"
            }
            MermaidTemplate::State => {
                "stateDiagram-v2\n  [*] --> Idle\n  Idle --> Running : start\n  Running --> Idle : stop\n"
            }
            MermaidTemplate::Er => {
                "erDiagram\n  CUSTOMER ||--o{ ORDER : places\n  ORDER ||--|{ LINE_ITEM : contains\n"
            }
            MermaidTemplate::Architecture => {
                "architecture-beta\n  group api(cloud)[API]\n  service db(database)[Database] in api\n  service server(server)[Server] in api\n  server:R -- L:db\n"
            }
        }
    }
}

/// The whole new buffer after appending `template` to `existing`.
///
/// The whole buffer rather than a suffix, because the empty and whitespace-only
/// cases have to be able to *drop* what was there: a template welded onto three
/// stray spaces starts the diagram keyword mid-line, which is the one thing
/// Mermaid will not forgive.
///
/// **Exactly one blank line separates the two, always.** The rule that
/// mattered was "never jam the appended block onto the last line"; normalising
/// rather than only patching the no-trailing-newline case is a deliberate step
/// past it, because the alternative makes one click produce two different
/// shapes depending on an invisible trailing character — and the shape it
/// produces for a buffer that already ends in a newline is the cramped one.
pub(crate) fn appended(existing: &str, template: &str) -> String {
    if existing.trim().is_empty() {
        return template.to_owned();
    }
    format!(
        "{}\n\n{}",
        existing.trim_end_matches(['\n', '\r']),
        template
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{DefaultMermaidRenderer, MermaidRenderer, MermaidTheme};

    const FLOWCHART: &str = "flowchart LR\n  A --> B\n";

    #[test]
    fn appending_to_an_empty_buffer_inserts_the_template_alone() {
        assert_eq!(appended("", FLOWCHART), FLOWCHART);
    }

    #[test]
    fn appending_to_a_whitespace_only_buffer_inserts_the_template_alone() {
        assert_eq!(appended("   \n\n\t ", FLOWCHART), FLOWCHART);
    }

    #[test]
    fn appending_to_a_buffer_with_no_trailing_newline_adds_a_blank_line() {
        assert_eq!(
            appended("sequenceDiagram\n  A->>B: Hi", FLOWCHART),
            format!("sequenceDiagram\n  A->>B: Hi\n\n{FLOWCHART}")
        );
    }

    #[test]
    fn appending_to_a_buffer_with_a_trailing_newline_adds_one_blank_line_not_two() {
        assert_eq!(
            appended("sequenceDiagram\n  A->>B: Hi\n", FLOWCHART),
            format!("sequenceDiagram\n  A->>B: Hi\n\n{FLOWCHART}")
        );
    }

    #[test]
    fn a_buffer_ending_in_several_newlines_still_gets_exactly_one_blank_line() {
        assert_eq!(
            appended("sequenceDiagram\n  A->>B: Hi\n\n\n\n", FLOWCHART),
            format!("sequenceDiagram\n  A->>B: Hi\n\n{FLOWCHART}")
        );
    }

    /// The failure the rule exists to prevent, stated once over the whole set
    /// rather than once per template: whatever was in the buffer, the
    /// template's first character must begin a line of its own.
    #[test]
    fn no_template_is_ever_jammed_onto_the_last_line() {
        let buffers = [
            "flowchart LR",
            "flowchart LR\n",
            "flowchart LR\n\n\n",
            "  indented",
        ];
        for template in MermaidTemplate::ALL {
            for buffer in buffers {
                let result = appended(buffer, template.source());
                let inserted_at = result
                    .rfind(template.source())
                    .expect("the template should survive being appended");
                assert!(
                    result[..inserted_at].ends_with("\n\n"),
                    "{template:?} after {buffer:?} did not start a line of its own: {result:?}"
                );
            }
        }
    }

    #[test]
    fn appending_never_loses_what_was_already_there() {
        let existing = "flowchart LR\n  A --> B";
        for template in MermaidTemplate::ALL {
            let result = appended(existing, template.source());
            assert!(result.starts_with(existing), "{result:?}");
            assert!(result.ends_with(template.source()), "{result:?}");
        }
    }

    /// [`MermaidTemplate::source`]'s claim, made true rather than remembered.
    /// A template that does not render is worse than no template at all, and
    /// the renderer is a pinned git dependency that moves under us.
    #[test]
    fn every_template_renders() {
        for template in MermaidTemplate::ALL {
            let output = DefaultMermaidRenderer
                .render(template.source(), MermaidTheme::Light)
                .unwrap_or_else(|error| panic!("{template:?} does not render: {error}"));
            assert!(output.svg.contains("<svg"), "{template:?}: {}", output.svg);
        }
    }
}
