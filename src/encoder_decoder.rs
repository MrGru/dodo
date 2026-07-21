use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::select::{Select, SelectState};
use gpui_component::{ActiveTheme, IndexPath, Sizable, StyledExt as _, h_flex, v_flex};

use base64::alphabet;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig, Engine as _};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};

/// Encoders are strict about padding; decoders accept padded or unpadded input
/// so pasted values from either convention round-trip.
const B64_CONFIG_DECODE: GeneralPurposeConfig =
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent);
const B64_STANDARD: GeneralPurpose = GeneralPurpose::new(&alphabet::STANDARD, B64_CONFIG_DECODE);
const B64_URL_SAFE: GeneralPurpose = GeneralPurpose::new(&alphabet::URL_SAFE, B64_CONFIG_DECODE);

/// The set of characters percent-encoded by `encodeURIComponent`: everything
/// except the unreserved ASCII characters.
const URI_COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Base64,
    Base64UrlSafe,
    Url,
    Hex,
    Jwt,
}

const FORMATS: [(Format, &str); 5] = [
    (Format::Base64, "Base64 (standard)"),
    (Format::Base64UrlSafe, "Base64 (URL-safe)"),
    (Format::Url, "URL percent-encoding"),
    (Format::Hex, "Hex"),
    (Format::Jwt, "JWT (decode only)"),
];

/// The encoder/decoder view: a format dropdown, an input editor, and an output
/// area. Base64/URL/Hex convert between the input and a single output editor in
/// either direction; JWT decodes the pasted token into three separate
/// header/payload/signature areas. Errors are surfaced in a banner rather than
/// silently producing empty output.
pub struct EncoderDecoder {
    format: Entity<SelectState<Vec<SharedString>>>,
    input: Entity<InputState>,
    output: Entity<InputState>,
    jwt_header: Entity<InputState>,
    jwt_payload: Entity<InputState>,
    jwt_signature: SharedString,
    error: Option<SharedString>,
}

impl EncoderDecoder {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let options: Vec<SharedString> = FORMATS
            .iter()
            .map(|(_, label)| SharedString::from(*label))
            .collect();
        let format =
            cx.new(|cx| SelectState::new(options, Some(IndexPath::default()), window, cx));

        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .soft_wrap(true)
                .placeholder("Paste the text or token to convert here.")
        });
        let output = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .soft_wrap(true)
                .placeholder("Result appears here.")
        });
        let jwt_header = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("json")
                .multi_line(true)
                .soft_wrap(true)
        });
        let jwt_payload = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("json")
                .multi_line(true)
                .soft_wrap(true)
        });

        Self {
            format,
            input,
            output,
            jwt_header,
            jwt_payload,
            jwt_signature: SharedString::default(),
            error: None,
        }
    }

    fn format(&self, cx: &App) -> Format {
        self.format
            .read(cx)
            .selected_index(cx)
            .and_then(|ip| FORMATS.get(ip.row).map(|(f, _)| *f))
            .unwrap_or(FORMATS[0].0)
    }

    fn encode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let source = self.input.read(cx).value().to_string();
        let result = match self.format(cx) {
            Format::Base64 => Ok(B64_STANDARD.encode(source.as_bytes())),
            Format::Base64UrlSafe => Ok(B64_URL_SAFE.encode(source.as_bytes())),
            Format::Url => Ok(utf8_percent_encode(&source, URI_COMPONENT).to_string()),
            Format::Hex => Ok(encode_hex(source.as_bytes())),
            // JWT has no encode direction (signing needs a key).
            Format::Jwt => Err("JWT is decode-only: no signing key is available.".to_string()),
        };
        self.apply(result, window, cx);
    }

    fn decode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let source = self.input.read(cx).value().to_string();
        let result = match self.format(cx) {
            Format::Base64 => decode_base64(&B64_STANDARD, &source),
            Format::Base64UrlSafe => decode_base64(&B64_URL_SAFE, &source),
            Format::Url => decode_url(&source),
            Format::Hex => decode_hex(&source),
            Format::Jwt => {
                self.decode_jwt(window, cx);
                return;
            }
        };
        self.apply(result, window, cx);
    }

    /// Writes a conversion result into the single-output editor, or shows its
    /// error message.
    fn apply(
        &mut self,
        result: Result<String, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(text) => {
                self.error = None;
                self.output
                    .update(cx, |state, cx| state.set_value(text, window, cx));
            }
            Err(message) => self.error = Some(SharedString::from(message)),
        }
        cx.notify();
    }

    fn decode_jwt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let token = self.input.read(cx).value().to_string();
        match split_jwt(token.trim()) {
            Ok((header, payload, signature)) => {
                self.error = None;
                self.jwt_signature = SharedString::from(signature);
                self.jwt_header
                    .update(cx, |state, cx| state.set_value(header, window, cx));
                self.jwt_payload
                    .update(cx, |state, cx| state.set_value(payload, window, cx));
            }
            Err(message) => {
                self.error = Some(SharedString::from(message));
                self.jwt_signature = SharedString::default();
            }
        }
        cx.notify();
    }

    fn error_banner(&self, cx: &App) -> Option<impl IntoElement> {
        self.error.clone().map(|error| {
            div()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().danger)
                .bg(cx.theme().danger.opacity(0.1))
                .text_color(cx.theme().danger)
                .text_sm()
                .px_3()
                .py_2()
                .child(error)
        })
    }

    fn editor(&self, state: &Entity<InputState>, cx: &App) -> impl IntoElement {
        div()
            .flex_1()
            .min_h_0()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .child(
                Input::new(state)
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(cx.theme().mono_font_size)
                    .size_full(),
            )
    }

    fn label(text: &'static str) -> impl IntoElement {
        div().text_sm().font_bold().child(text)
    }
}

impl Render for EncoderDecoder {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_jwt = self.format(cx) == Format::Jwt;

        v_flex()
            .size_full()
            .gap_3()
            .child(
                h_flex()
                    .items_center()
                    .gap_3()
                    .child(div().text_sm().child("Format:"))
                    .child(Select::new(&self.format).small().w(px(200.)))
                    .when(!is_jwt, |this| {
                        this.child(
                            Button::new("encode")
                                .primary()
                                .small()
                                .label("Encode")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.encode(window, cx);
                                })),
                        )
                        .child(
                            Button::new("decode")
                                .small()
                                .label("Decode")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.decode(window, cx);
                                })),
                        )
                    })
                    .when(is_jwt, |this| {
                        this.child(
                            Button::new("decode-jwt")
                                .primary()
                                .small()
                                .label("Decode JWT")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.decode(window, cx);
                                })),
                        )
                    }),
            )
            .children(self.error_banner(cx))
            .child(Self::label("Input"))
            .child(self.editor(&self.input, cx))
            .when(!is_jwt, |this| {
                this.child(Self::label("Output"))
                    .child(self.editor(&self.output, cx))
            })
            .when(is_jwt, |this| {
                this.child(Self::label("Header"))
                    .child(self.editor(&self.jwt_header, cx))
                    .child(Self::label("Payload"))
                    .child(self.editor(&self.jwt_payload, cx))
                    .child(Self::label("Signature (not verified)"))
                    .child(
                        div()
                            .rounded(cx.theme().radius)
                            .border_1()
                            .border_color(cx.theme().border)
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_size(cx.theme().mono_font_size)
                            .px_3()
                            .py_2()
                            .child(self.jwt_signature.clone()),
                    )
            })
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn decode_hex(source: &str) -> Result<String, String> {
    let digits: Vec<char> = source.chars().filter(|c| !c.is_whitespace()).collect();
    if digits.len() % 2 != 0 {
        return Err(format!(
            "Invalid hex: expected an even number of digits, got {}.",
            digits.len()
        ));
    }

    let mut bytes = Vec::with_capacity(digits.len() / 2);
    for (index, pair) in digits.chunks(2).enumerate() {
        let hi = hex_value(pair[0], index * 2)?;
        let lo = hex_value(pair[1], index * 2 + 1)?;
        bytes.push((hi << 4) | lo);
    }
    bytes_to_string(bytes)
}

fn hex_value(c: char, position: usize) -> Result<u8, String> {
    c.to_digit(16)
        .map(|d| d as u8)
        .ok_or_else(|| format!("Invalid hex: '{c}' at position {position} is not a hex digit."))
}

fn decode_base64(engine: &GeneralPurpose, source: &str) -> Result<String, String> {
    // Base64 is often pasted with wrapping newlines; they are not data.
    let cleaned: String = source.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = engine
        .decode(cleaned.as_bytes())
        .map_err(|err| format!("Invalid base64: {err}"))?;
    bytes_to_string(bytes)
}

fn decode_url(source: &str) -> Result<String, String> {
    // `percent_decode_str` passes malformed sequences through untouched, so
    // validate them up front to be able to report the problem.
    let bytes = source.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte != b'%' {
            continue;
        }
        let valid = bytes
            .get(index + 1..index + 3)
            .is_some_and(|pair| pair.iter().all(u8::is_ascii_hexdigit));
        if !valid {
            return Err(format!(
                "Invalid percent-encoding: '%' at position {index} is not followed by two hex digits."
            ));
        }
    }

    percent_decode_str(source)
        .decode_utf8()
        .map(|decoded| decoded.into_owned())
        .map_err(|err| format!("Invalid percent-encoding: {err}"))
}

fn bytes_to_string(bytes: Vec<u8>) -> Result<String, String> {
    String::from_utf8(bytes)
        .map_err(|err| format!("Decoded bytes are not valid UTF-8 text: {err}"))
}

/// Splits a JWT into its three parts, returning the pretty-printed header and
/// payload JSON plus the raw signature. No signature verification is attempted:
/// this is an inspection tool and no key is available.
fn split_jwt(token: &str) -> Result<(String, String, String), String> {
    if token.is_empty() {
        return Err("Invalid JWT: the input is empty.".to_string());
    }

    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(format!(
            "Invalid JWT: expected 3 dot-separated parts, got {}.",
            parts.len()
        ));
    }

    let header = decode_jwt_json(parts[0], "header")?;
    let payload = decode_jwt_json(parts[1], "payload")?;
    Ok((header, payload, parts[2].to_string()))
}

fn decode_jwt_json(part: &str, name: &str) -> Result<String, String> {
    let bytes = B64_URL_SAFE
        .decode(part.as_bytes())
        .map_err(|err| format!("Invalid JWT: the {name} is not valid base64url ({err})."))?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|err| format!("Invalid JWT: the {name} is not valid JSON ({err})."))?;
    serde_json::to_string_pretty(&value)
        .map_err(|err| format!("Invalid JWT: could not render the {name} ({err})."))
}
