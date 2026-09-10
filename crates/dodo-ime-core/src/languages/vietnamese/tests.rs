//! The Vietnamese engine's behaviour, stated as tables.
//!
//! The corpus round trip in [`super::corpus`] proves the engine reproduces
//! several hundred real words. This file states the *rules* — one table per
//! rule, hand-written, so that a regression names the rule it broke rather than
//! dumping three hundred failures.

use super::corpus;
use super::{InputScheme, OutputMode, TonePlacement, VietnameseConfig, VietnameseEngine};
use crate::core::{EngineAction, Key, KeyEvent, LanguageEngine, LanguageId, Modifiers};
use crate::languages::vietnamese::unicode::nfc;
use crate::testing::{Host, press, type_keys, type_keys_uncommitted};

fn engine() -> VietnameseEngine {
    VietnameseEngine::default()
}

fn configured(config: VietnameseConfig) -> VietnameseEngine {
    VietnameseEngine::new(config)
}

/// Type `keys` into a fresh default engine and return what the document holds.
fn telex(keys: &str) -> String {
    type_keys(&mut engine(), keys)
}

fn vni(keys: &str) -> String {
    type_keys(
        &mut configured(VietnameseConfig {
            scheme: InputScheme::Vni,
            ..VietnameseConfig::default()
        }),
        keys,
    )
}

fn check(cases: &[(&str, &str)], typed: impl Fn(&str) -> String) {
    let mut failures = Vec::new();
    for (keys, expected) in cases {
        let actual = typed(keys);
        if actual != *expected {
            failures.push(format!("{keys} -> {actual:?}, wanted {expected:?}"));
        }
    }
    assert!(failures.is_empty(), "\n  {}", failures.join("\n  "));
}

fn composition(text: &str) -> Vec<EngineAction> {
    vec![EngineAction::SetComposition {
        text: text.into(),
        cursor: text.chars().count(),
        selection: None,
    }]
}

fn action_stream(engine: &mut VietnameseEngine, keys: &str) -> Vec<Vec<EngineAction>> {
    keys.chars()
        .map(|key| engine.process_key(&KeyEvent::character(key)).actions)
        .collect()
}

// ---------------------------------------------------------------- the spec

/// The five worked examples from the specification, hand-written so that a
/// broken key generator in [`super::corpus`] cannot hide behind a matching bug
/// in the engine.
#[test]
fn the_specifications_worked_examples() {
    check(
        &[
            ("tieengs", "tiếng"),
            ("Vieetj", "Việt"),
            ("ddawng", "đăng"),
            ("dduwowngf", "đường"),
            ("nguyeenx", "nguyễn"),
            ("Nguyeenx", "Nguyễn"),
            ("chuyeen", "chuyên"),
            // Modifiers search the current nucleus, not merely the last key,
            // so equivalent Telex spellings converge.
            ("hoiw", "hơi"),
            ("thienej", "thiện"),
            ("thieenj", "thiện"),
        ],
        telex,
    );
}

#[test]
fn telex_rewrites_each_intermediate_state() {
    let mut normal = engine();
    assert_eq!(
        action_stream(&mut normal, "thuwowng"),
        ["t", "th", "thu", "thư", "thưo", "thươ", "thươn", "thương"].map(composition)
    );

    let mut incremental = engine();
    assert_eq!(
        action_stream(&mut incremental, "thuow"),
        ["t", "th", "thu", "thuo", "thuơ"].map(composition)
    );
    assert_eq!(action_stream(&mut incremental, "n"), [composition("thươn")]);
    assert_eq!(
        action_stream(&mut incremental, "g"),
        [composition("thương")]
    );

    let mut coda_first = engine();
    assert_eq!(
        action_stream(&mut coda_first, "thuonw"),
        ["t", "th", "thu", "thuo", "thuon", "thươn"].map(composition)
    );
    check(
        &[
            ("thuowc", "thươc"),
            ("thuowch", "thươch"),
            ("thuowm", "thươm"),
            ("thuowng", "thương"),
            ("thuownh", "thươnh"),
            ("thuowp", "thươp"),
            ("thuowt", "thươt"),
            ("thuowi", "thươi"),
            // Tone before/after both the modifier and coda converges.
            ("thuowrn", "thưởn"),
            ("thuornw", "thưởn"),
            ("thuownr", "thưởn"),
            ("thuonrw", "thưởn"),
        ],
        telex,
    );

    check(
        &[("thuo7", "thuơ"), ("thuo7n", "thươn"), ("thuon7", "thươn")],
        vni,
    );

    let mut reopened = engine();
    action_stream(&mut reopened, "thuown");
    let backspace = reopened.process_key(&KeyEvent::special(Key::Backspace));
    assert_eq!(backspace.actions, composition("thuơ"));
    assert_eq!(action_stream(&mut reopened, "n"), [composition("thươn")]);
}

/// The reported failure, key by key.
///
/// Read the first line as Telex rather than as English and it is not
/// surprising: a `w` with no vowel behind it types `ư`, so the word starts as
/// `ư` and stays that way until the second `w` recognises its own earlier
/// letter and puts the key back where it stands.
#[test]
fn telex_puts_an_earlier_w_back_where_it_stands() {
    let mut typed = engine();
    assert_eq!(
        action_stream(&mut typed, "window"),
        ["ư", "ưi", "ưin", "wind", "windo", "window"].map(composition)
    );

    // The same recovery reached from the other side. `ww` settles the leading
    // `w` as literal; after the `i` nucleus, later targetless `w` keys are
    // literal too rather than synthetic `ư` letters.
    let mut literal = engine();
    assert_eq!(
        action_stream(&mut literal, "wwindoww"),
        ["ư", "w", "wi", "win", "wind", "windo", "window", "windoww"].map(composition)
    );
}

#[test]
fn telex_modifier_and_tone_rewrites_are_ordered() {
    let mut lower = engine();
    assert_eq!(
        action_stream(&mut lower, "ddd"),
        ["d", "đ", "dd"].map(composition)
    );
    let mut upper = engine();
    assert_eq!(
        action_stream(&mut upper, "DDD"),
        ["D", "Đ", "DD"].map(composition)
    );

    let mut tone = engine();
    assert_eq!(
        action_stream(&mut tone, "toasn"),
        ["t", "to", "toa", "toá", "toán"].map(composition)
    );
}

// ------------------------------------------------------- letters and marks

#[test]
fn the_doubled_vowels_and_the_w_marks() {
    check(
        &[
            ("aa", "â"),
            ("ee", "ê"),
            ("oo", "ô"),
            ("aw", "ă"),
            ("ow", "ơ"),
            ("uw", "ư"),
            ("dd", "đ"),
            ("w", "ư"),
            ("W", "Ư"),
            ("uow", "ươ"),
            ("uwow", "ươ"),
        ],
        telex,
    );
}

/// The stroke reaches back over the rest of the word, like every other
/// modifier here — `did` is `đi`, not `did`.
///
/// It was once the only modifier that demanded adjacency, which made `ddi`
/// work and `did` type three literal letters. What the rule actually says is
/// *the syllable's **initial** letter is a `d`*, which is why `add` is still
/// `add`, and why VNI's `9` — asking the same shared question — never had the
/// defect.
///
/// Where it reaches back *from* is only half of it: the two tables below state
/// what a stroke from a distance has to be plausible over, and what an adjacent
/// `dd` outranks.
#[test]
fn the_stroke_reaches_back_to_the_syllables_initial_d() {
    check(
        &[
            ("did", "đi"),
            ("ddi", "đi"),
            // A second stroke key undoes the first and types itself, which is
            // the shared repeated-modifier rule, not a stroke rule.
            ("didd", "did"),
            // Not the initial letter, so not a stroke.
            ("add", "add"),
            // Case follows the letter that was typed, never the modifier's.
            ("Did", "Đi"),
            ("DID", "ĐI"),
            ("dId", "đI"),
            ("DIDD", "DID"),
            // The real words this exists for, typed the natural way round.
            ("dungd", "đung"),
            ("dawngd", "đăng"),
            ("duwowngfd", "đường"),
        ],
        telex,
    );
}

/// The cost of the rule above, stated rather than hidden: a Latin word whose
/// keys spell a *valid* Vietnamese syllable is composed, because the
/// word-boundary restore in [`super::rules`] only rescues syllables that are
/// invalid. `đô` is perfectly good Vietnamese, so `dodo` stays `đô` through the
/// space that ends it.
///
/// This is what Telex does — Unikey behaves identically — and it is not new
/// with the stroke: `dis` has always typed `dí`. Words that do *not* spell a
/// Vietnamese syllable are restored as typed, which covers most of them.
#[test]
fn a_latin_word_that_spells_a_valid_syllable_is_still_composed() {
    check(
        &[
            ("dodo", "đô"),
            ("dodo ", "đô "),
            ("dad ", "đa "),
            // Restored, because the keys do not spell a Vietnamese syllable.
            ("didnt ", "didnt "),
            ("dodgy ", "dodgy "),
            ("odd ", "odd "),
        ],
        telex,
    );
}

/// The other half of that rule: a stroke arriving **from a distance** is an
/// inference from the shape of the word, and an inference is only drawn over
/// something that could be a Vietnamese syllable. `did` is `đi` because `di`
/// is one; `download` keeps the `d` it ends with because `đownloa` is not.
///
/// The keys before the stroke are what decides it, never a list of English
/// words — see `rules::is_valid_syllable`, which the doubled vowel and the
/// tone keys have always asked and which the stroke used to skip.
#[test]
fn a_stroke_reaching_back_needs_a_possible_vietnamese_syllable() {
    check(
        &[
            ("download", "download"),
            ("dcd", "dcd"),
            ("dmd", "dmd"),
            ("dvd", "dvd"),
            ("dnd", "dnd"),
            // Still believed, because these are possible syllables.
            ("did", "đi"),
            ("dungd", "đung"),
            ("dad ", "đa "),
        ],
        telex,
    );

    // Structural, not a consequence of the spell-check restore: with the
    // setting off the horn still fires — that is the documented cost of
    // turning it off — but the trailing `d` is no longer eaten.
    let mut off = configured(VietnameseConfig {
        spell_check: false,
        ..VietnameseConfig::default()
    });
    assert_eq!(type_keys(&mut off, "download"), "dơnload");
}

/// An adjacent `dd` is a *statement*, not an inference, and it survives both
/// the letters that follow it and the spell-check restore: `ddm` is `đm` and
/// `ddc` is `đc`, the abbreviations Vietnamese writes them as.
///
/// The restore exists to rescue words nobody stated anything about, so the
/// doubled *vowel* keeps it — `book` is not `bôk` — and only the stroke, the
/// one mark that spells a letter of the Vietnamese alphabet, takes the raw
/// keys out of play.
#[test]
fn an_explicit_dd_keeps_its_stroke_whatever_follows() {
    check(
        &[
            ("ddm", "đm"),
            ("ddc", "đc"),
            ("ddt", "đt"),
            ("ddma", "đma"),
            ("Ddm", "đm"),
            ("dDm", "Đm"),
            // A third `d` is still the shared undo rule, keys and all.
            ("ddd", "dd"),
            // The doubled vowel states its case just as loudly and is still
            // handed back, because `bôk` is nobody's Vietnamese.
            ("book", "book"),
            ("look", "look"),
            ("food", "food"),
        ],
        telex,
    );
}

#[test]
fn the_vni_digits_do_the_same_things() {
    check(
        &[
            ("a6", "â"),
            ("e6", "ê"),
            ("o6", "ô"),
            ("a8", "ă"),
            ("o7", "ơ"),
            ("u7", "ư"),
            ("d9", "đ"),
            ("uo7", "ươ"),
            ("u7o7", "ươ"),
        ],
        vni,
    );
}

/// VNI's `9` has always asked the shared "which letter takes a stroke"
/// question, so it reached back from anywhere in the word before Telex did.
/// Kept as its own table so the two schemes cannot drift apart again.
#[test]
fn the_vni_stroke_also_reaches_back_to_the_initial_d() {
    check(
        &[
            ("di9", "đi"),
            ("d9i", "đi"),
            ("di9nh", "đinh"),
            ("du9ng", "đung"),
            // Not the initial letter: the digit has nowhere to land and is
            // typed as a digit.
            ("ad9", "ad9"),
            ("a9", "a9"),
            // And nowhere sensible to land: a `9` reaching back over letters
            // that cannot be a syllable is the digit nine, not a stroke, so it
            // is typed rather than eaten.
            ("dvd9", "dvd9"),
            ("dcd9", "dcd9"),
            ("dmd9", "dmd9"),
            // A digit is never part of a word, so `d9` states the stroke the
            // way Telex's `dd` does and keeps it: `đm`, not `d9m`.
            ("d9m", "đm"),
            ("d9c", "đc"),
            ("d9t", "đt"),
            ("d9ma", "đma"),
        ],
        vni,
    );
}

#[test]
fn the_five_tones_and_the_level_tone() {
    check(
        &[
            ("ma", "ma"),
            ("mas", "má"),
            ("maf", "mà"),
            ("mar", "mả"),
            ("max", "mã"),
            ("maj", "mạ"),
        ],
        telex,
    );
    check(
        &[
            ("ma", "ma"),
            ("ma1", "má"),
            ("ma2", "mà"),
            ("ma3", "mả"),
            ("ma4", "mã"),
            ("ma5", "mạ"),
        ],
        vni,
    );
}

#[test]
fn z_and_zero_take_the_tone_off() {
    check(
        &[("masz", "ma"), ("mafz", "ma"), ("tieengsz", "tiêng")],
        telex,
    );
    check(&[("ma10", "ma"), ("tie6ng10", "tiêng")], vni);
}

/// A tone key typed before the rest of the syllable still lands correctly,
/// because the tone is a fact about the syllable rather than about a letter.
#[test]
fn a_tone_may_be_typed_anywhere_in_the_syllable() {
    check(
        &[
            ("toansr", "toản"),
            ("torans", "toán"),
            ("tieesng", "tiếng"),
            ("tieengs", "tiếng"),
        ],
        telex,
    );
}

// -------------------------------------------------------------------- undo

/// `aaa` types `aa`, `ss` types `s`, `ddd` types `dd`. The rule is stated once
/// in the shared layer, so it holds for both schemes.
#[test]
fn a_repeated_modifier_undoes_itself_and_types_its_key() {
    check(
        &[
            ("aaa", "aa"),
            ("eee", "ee"),
            ("ooo", "oo"),
            ("ddd", "dd"),
            ("caa", "câ"),
            ("caaa", "caa"),
            ("cass", "cas"),
            ("maff", "maf"),
            ("marr", "mar"),
            ("maxx", "max"),
            ("majj", "maj"),
            ("caf f", "cà f"),
            ("cassa", "casa"),
            ("aww", "aw"),
            ("oww", "ow"),
            // The first `w` is the whole source of `ư`, not a typed `u` with
            // a later mark. Repeating that source removes the whole letter.
            ("ww", "w"),
            ("wW", "W"),
            ("Ww", "w"),
            ("WW", "W"),
            // But when the `u` was physical, it remains after the horn is
            // undone; source provenance distinguishes this from `ww`.
            ("uww", "uw"),
            // The ordinary mark and tone repetitions retain their source
            // letters and continue to type the repeated key literally.
            ("aaa", "aa"),
            ("ddd", "dd"),
            ("mass", "mas"),
        ],
        telex,
    );
}

/// The same rule when the two presses are not next to each other.
///
/// A standalone `w` is a whole letter the key made, so pressing it again takes
/// that letter back wherever it stands — and whatever arrived in between, which
/// is the part that used to go wrong in two different ways. `ưindo` + `w`
/// removed the `ư` and let the caller append the restored `w`, reordering the
/// word into `indow`; and a `ư` that had fallen outside the nucleus was not
/// recognised as cancellable at all, so `windoư` + `w` grew a second one.
///
/// Both readings come from provenance alone — which key made this letter, and
/// how many letters have arrived since. No word list, no rendered text.
///
/// Once that cancellation has left a literal `w` — a letter Vietnamese does not
/// have — the current word has declared itself English (the captain's call,
/// 2026-09-10): later keys in it stay literal rather than re-opening the horn,
/// so `www` is `ww` and not `wư`. See [`VietnameseEngine::normalize`].
#[test]
fn a_repeated_standalone_w_takes_back_the_letter_it_made_wherever_it_is() {
    check(
        &[
            // Alone, and as one gesture: two presses, one letter. A third press
            // does not re-open the horn — the literal `w` already made this word
            // English — so the counts run `ư`, `w`, `ww`, `www`.
            ("w", "ư"),
            ("ww", "w"),
            ("www", "ww"),
            ("wwww", "www"),
            // At the start, with the rest of the word in between: two presses,
            // two letters, because the second `w` is the next letter of a word
            // rather than a correction of the first.
            ("wiw", "wiw"),
            ("window", "window"),
            ("windows", "windows"),
            ("widow", "widow"),
            ("willow", "willow"),
            ("WINDOW", "WINDOW"),
            ("Window", "Window"),
            // In the middle and at the end. Once `ww` makes the leading `w`
            // literal, a later targetless `w` after the `i` nucleus is literal.
            ("wwindow", "window"),
            ("wwindoww", "windoww"),
            // After text that has already proved not to be Vietnamese.
            ("sww", "sw"),
            ("twnw", "twnw"),
        ],
        telex,
    );
}

/// The captain's per-word English intent (2026-09-10): once a run holds a
/// literal letter Vietnamese does not have (`f`, `j`, `w`, `z`) — reached by
/// cancelling a control (`ww` → `w`) or by a modifier with nowhere to land —
/// the current word has declared itself English, and every later key in it
/// stays literal. The reach-back horn `w` is the control this closes: an invalid
/// syllable already refused the tone keys, but the horn could still reach a
/// vowel and mark it, so `wwarow` used to become `wăro`.
#[test]
fn a_word_that_has_shown_a_foreign_letter_stops_interpreting_telex() {
    check(
        &[
            // The double-w cancellation is strong English evidence; the later
            // keys, horn `w` included, are literal.
            ("wwarow", "warow"),
            ("wwas", "was"),
            ("wwaf", "waf"),
            ("wwix", "wix"),
            ("wwao", "wao"),
            ("uww", "uw"),
            // A targetless modifier reaches the same literal letter and latches
            // exactly as the cancellation does.
            ("sww", "sw"),
            ("twnw", "twnw"),
        ],
        telex,
    );

    // Guardrail — composition is untouched for a word that never showed a
    // foreign letter, and the horn that *makes* `ư`/`ơ` is Vietnamese, not
    // foreign: it is a `u`/`o` with a mark, so it never trips this rule.
    check(
        &[
            ("tieengs", "tiếng"),
            ("tuwr", "tử"),
            ("dduonwg", "đương"),
            ("muwax", "mữa"),
        ],
        telex,
    );
}

/// The bracket shortcuts are Telex whole-letter sources, just like bare `w`.
/// Repeating one takes its letter back and types the key literally; a third
/// press starts a new shortcut after that punctuation boundary. The visible
/// composition must be removed in both host output modes when the cancelled
/// letter was the whole syllable.
#[test]
fn a_repeated_bracket_shortcut_takes_back_the_letter_it_made() {
    let cases = &[
        ("[", "ơ"),
        ("[[", "["),
        ("[[[", "[ơ"),
        ("]", "ư"),
        ("]]", "]"),
        ("]]]", "]ư"),
        ("{{", "{"),
        ("{{{", "{Ơ"),
        ("}}", "}"),
        ("}}}", "}Ư"),
    ];
    check(cases, telex);
    check(cases, |keys| {
        type_keys(
            &mut configured(VietnameseConfig {
                output: OutputMode::Direct,
                ..VietnameseConfig::default()
            }),
            keys,
        )
    });

    // These shortcuts belong to Telex, not VNI.
    check(
        &[("[[", "[["), ("]]", "]]"), ("{{", "{{"), ("}}", "}}")],
        vni,
    );
}

/// The distinction the whole fix rests on: `ư` the user typed and `ư` dodo
/// rendered are different states, and only the second is a `w` to be taken
/// back.
///
/// Both words below show `ưi` before their last key. `wiw` gets its `w` back,
/// because a `w` made that letter outright; `uwiw` keeps the `u` the user
/// actually typed and only loses the horn, exactly as `uww` does. Nothing that
/// read the rendered string could tell the two apart.
#[test]
fn one_horn_command_on_uo_undoes_the_whole_pair() {
    check(
        &[
            ("uoww", "uow"),
            ("dduoww", "đuow"),
            ("thuoww", "thuow"),
            ("uo77", "uo7"),
        ],
        |keys| {
            if keys.contains('7') {
                vni(keys)
            } else {
                telex(keys)
            }
        },
    );
}

#[test]
fn a_typed_u_and_a_w_that_rendered_u_horn_undo_differently() {
    let mut made_by_w = engine();
    assert_eq!(type_keys_uncommitted(&mut made_by_w, "wi"), "ưi");
    let mut typed_as_u = engine();
    assert_eq!(type_keys_uncommitted(&mut typed_as_u, "uwi"), "ưi");

    check(&[("wiw", "wiw"), ("uwiw", "uiw"), ("uww", "uw")], telex);
}

/// The VNI half of the same rule. A digit cannot join a syllable, so the
/// syllable ends and the digit is typed by the application: `a11` is `a1`.
/// Unlike Telex's bare `w`, no VNI digit creates a whole marked letter, so
/// `u77` still retains its physical `u`.
#[test]
fn a_repeated_vni_digit_undoes_itself_and_types_its_digit() {
    check(
        &[
            ("a66", "a6"),
            ("ma11", "ma1"),
            ("ma22", "ma2"),
            ("ma33", "ma3"),
            ("ma44", "ma4"),
            ("ma55", "ma5"),
            ("d99", "d9"),
            ("u77", "u7"),
            ("a88", "a8"),
            ("a666", "a66"),
            ("ma111", "ma11"),
            ("ma11n", "ma1n"),
            // The literal digit finishes the old syllable. Later letters cannot
            // let a control reach back across it; a later digit starts fresh.
            ("ma11w", "ma1w"),
            ("ma11a8", "ma1ă"),
        ],
        vni,
    );
}

/// Undoing a tone and then setting a different one is not an undo at all.
#[test]
fn a_different_tone_replaces_rather_than_undoing() {
    check(&[("masf", "mà"), ("masfx", "mã")], telex);
    check(&[("ma12", "mà"), ("ma124", "mã")], vni);
}

// --------------------------------------------------------- tone placement

#[test]
fn tone_placement_across_the_compound_vowels() {
    check(
        &[
            // oa, oe, uy — modern placement, the default.
            ("hoaf", "hoà"),
            ("khoer", "khoẻ"),
            ("thuyr", "thuỷ"),
            // ia, ua, ưa — the first vowel.
            ("biaf", "bìa"),
            ("muas", "múa"),
            ("muwas", "mứa"),
            // iê, yê, uô, ươ — the vowel with the diacritic.
            ("chieeuf", "chiều"),
            ("yeeus", "yếu"),
            ("chuoois", "chuối"),
            ("nguwowif", "người"),
            // Three vowels: the middle one.
            ("ngoaif", "ngoài"),
            ("xoays", "xoáy"),
            ("khuyur", "khuỷu"),
        ],
        telex,
    );
}

#[test]
fn a_final_consonant_moves_the_tone_onto_the_last_vowel() {
    check(
        &[
            ("toans", "toán"),
            ("hoanf", "hoàn"),
            ("Huyfnh", "Huỳnh"),
            ("loanj", "loạn"),
            // And the tone stays put when the nucleus is already decided by a
            // diacritic.
            ("tieengs", "tiếng"),
            ("cuoongs", "cuống"),
            ("nguyeenx", "nguyễn"),
        ],
        telex,
    );
}

/// The one contested case, both ways, and the proof that the switch reaches
/// nothing else.
#[test]
fn the_tone_placement_setting_changes_oa_oe_and_uy_and_nothing_else() {
    let traditional = || {
        configured(VietnameseConfig {
            tone_placement: TonePlacement::Traditional,
            ..VietnameseConfig::default()
        })
    };

    for (keys, modern, old) in [
        ("hoaf", "hoà", "hòa"),
        ("khoer", "khoẻ", "khỏe"),
        ("thuyr", "thuỷ", "thủy"),
        ("xoas", "xoá", "xóa"),
    ] {
        assert_eq!(telex(keys), modern, "{keys}, modern");
        assert_eq!(
            type_keys(&mut traditional(), keys),
            old,
            "{keys}, traditional"
        );
    }

    for keys in [
        "tieengs",
        "dduwowngf",
        "ngoaif",
        "toans",
        "chuoois",
        "quys",
        "giair",
        "nguyeenx",
    ] {
        assert_eq!(
            telex(keys),
            type_keys(&mut traditional(), keys),
            "{keys} should not depend on the placement style"
        );
    }
}

/// `quả`, not `qủa`; `giả`, not `gỉa`; and `gì`, where the `i` is the nucleus
/// after all.
#[test]
fn qu_and_gi_keep_their_glide_out_of_the_nucleus() {
    check(
        &[
            ("quar", "quả"),
            ("quys", "quý"),
            ("quyeenf", "quyền"),
            ("quoocs", "quốc"),
            ("quees", "quế"),
            ("giar", "giả"),
            ("giair", "giải"),
            ("gieengs", "giếng"),
            ("giwx", "giữ"),
            ("gif", "gì"),
            ("ginf", "gìn"),
        ],
        telex,
    );
}

// -------------------------------------------------------- capitalization

#[test]
fn capitalization_survives_every_transform() {
    check(
        &[
            ("Vieetj", "Việt"),
            ("VIEETJ", "VIỆT"),
            ("DDuwowngf", "Đường"),
            ("DDUWOWNGF", "ĐƯỜNG"),
            ("Nguyeenx", "Nguyễn"),
            ("NGUYEENX", "NGUYỄN"),
            ("Haf Nooij", "Hà Nội"),
            ("HAF NOOIJ", "HÀ NỘI"),
            ("DDi", "Đi"),
            ("ddi DDi", "đi Đi"),
        ],
        telex,
    );
    // The case of a diacritic follows the *letter*, not the digit that
    // marked it: `VIe6t5` really is `VIệt`.
    check(
        &[
            ("VIE6T5", "VIỆT"),
            ("VIe6t5", "VIệt"),
            ("D9u7o7ng2", "Đường"),
        ],
        vni,
    );
}

/// # The rule
///
/// **A modifier key that is not the letter it marks decides which diacritic and
/// nothing else.** Telex's `w`, the five tone letters and every VNI digit are
/// all of them: shift is how a typist reaches `S` for *sắc* on a caps-locked
/// word, not an opinion about the vowel underneath, and a digit has no case at
/// all to leak in the first place. So `Aw` is `Ă`, `mAs` is `mÁ` and `D9` is
/// `Đ`.
///
/// A doubled *letter* key is the other half of the rule and has its own table
/// in `a_doubled_letter_key_states_the_case_of_the_letter_it_marks`, next to
/// the reasoning. The two together are the whole of it.
///
/// The three exceptions all have the same shape: **when the modifier key
/// becomes a letter itself, its own case is the only case there is.** Telex's
/// bare `w` types `ư` outright, so `W` types `Ư`; the bracket shortcuts type
/// `ơ`/`ư` and `{`/`}` type `Ơ`/`Ư`; and a repeated modifier that undoes itself
/// types its own key, so `cAsS` ends `cAS`. Those are covered below too, next
/// to the rule they look like a violation of.
#[test]
fn a_modifier_keys_own_case_never_reaches_the_letter_it_marks() {
    check(
        &[
            // `w` landing on a vowel is a breve or a horn, never a letter.
            ("aw", "ă"),
            ("aW", "ă"),
            ("Aw", "Ă"),
            ("AW", "Ă"),
            ("ow", "ơ"),
            ("oW", "ơ"),
            ("Ow", "Ơ"),
            ("OW", "Ơ"),
            ("uw", "ư"),
            ("uW", "ư"),
            ("Uw", "Ư"),
            ("UW", "Ư"),
            // One horn key marking a bare `uo` pair keeps each vowel's own
            // case, rather than levelling them.
            ("uow", "ươ"),
            ("uOw", "ưƠ"),
            ("Uow", "Ươ"),
            ("UOW", "ƯƠ"),
            // And a tone key, which lands on a vowel chosen by position.
            ("mas", "má"),
            ("maS", "má"),
            ("Mas", "Má"),
            ("mAs", "mÁ"),
            ("MAS", "MÁ"),
        ],
        telex,
    );

    // Mixed case inside one syllable. Every doubled vowel below agrees with
    // itself, so only a *leaked* modifier case could show.
    check(
        &[
            ("tiEEngs", "tiẾng"),
            ("tIeengS", "tIếng"),
            ("nguyEEnx", "nguyỄn"),
            ("NGUYeenX", "NGUYễn"),
            ("duOwngf", "dưỜng"),
            ("dUowngf", "dƯờng"),
            ("DuOwNgF", "DưỜNg"),
            ("chuOOis", "chuỐi"),
            ("hOaf", "hOà"),
            ("nGOAIF", "nGOÀI"),
        ],
        telex,
    );

    // A digit carries no case, so VNI can only ever report the letter's own.
    check(
        &[
            ("a6", "â"),
            ("A6", "Â"),
            ("e6", "ê"),
            ("E6", "Ê"),
            ("o6", "ô"),
            ("O6", "Ô"),
            ("a8", "ă"),
            ("A8", "Ă"),
            ("o7", "ơ"),
            ("O7", "Ơ"),
            ("u7", "ư"),
            ("U7", "Ư"),
            ("d9", "đ"),
            ("D9", "Đ"),
            ("d9I", "đI"),
            ("D9i", "Đi"),
            ("uo7", "ươ"),
            ("uO7", "ưƠ"),
            ("Uo7", "Ươ"),
            ("u7O7", "ưƠ"),
            ("tiE6ng1", "tiẾng"),
            ("TIe6NG1", "TIếNG"),
            ("nguyE6n4", "nguyỄn"),
            ("D9U7O7NG2", "ĐƯỜNG"),
            ("mA1", "mÁ"),
            ("Ma1", "Má"),
        ],
        vni,
    );
}

/// # The other half of the rule
///
/// **A doubled letter key states the case of the letter it marks.** `dd`, `aa`,
/// `ee` and `oo` are not really modifier keys: each one is a second press of
/// the very letter it decorates, so its own shift is the user's latest word on
/// that letter and it wins outright. `dD` is `Đ` and `Dd` is `đ`; `aA` is `Â`
/// and `Aa` is `â`.
///
/// Two conditions have to hold, and each has its own block below.
///
/// - **The key has to be that letter.** `w` and the tone letters are not, so
///   they leave the case alone — that is the table in
///   [`a_modifier_keys_own_case_never_reaches_the_letter_it_marks`]. VNI cannot
///   express this rule at all, because a digit has no case: `Dd` is `đ` while
///   `D9` is `Đ`, and that is the one place the two schemes disagree on
///   purpose.
/// - **Nothing may have been typed since.** The two presses are one gesture
///   only when they are adjacent. A stroke that reaches back over a word marks
///   a letter whose case the user settled several keys ago — `Did` is `Đi`, and
///   a *shifted* reach-back key does not recase it either.
///
/// Undoing the mark undoes the override with it, so no keystroke's own case is
/// lost to a key that was taken back: `Ddd` is `Dd`.
#[test]
fn a_doubled_letter_key_states_the_case_of_the_letter_it_marks() {
    check(
        &[
            // The captain's table, exactly.
            ("dd", "đ"),
            ("dD", "Đ"),
            ("Dd", "đ"),
            ("DD", "Đ"),
            // And the same reading for every other doubled letter.
            ("aa", "â"),
            ("aA", "Â"),
            ("Aa", "â"),
            ("AA", "Â"),
            ("ee", "ê"),
            ("eE", "Ê"),
            ("Ee", "ê"),
            ("EE", "Ê"),
            ("oo", "ô"),
            ("oO", "Ô"),
            ("Oo", "ô"),
            ("OO", "Ô"),
            // In a word rather than alone.
            ("dDi", "Đi"),
            ("Ddi", "đi"),
            ("cAan", "cân"),
            ("caAn", "cÂn"),
            ("cAAn", "cÂn"),
            ("tiEEngs", "tiẾng"),
        ],
        telex,
    );

    // A modifier that reached back over the word is applied from a distance and
    // never recases its target, however it was shifted. This is the block that
    // fails if the rule is implemented as "the latest key always wins".
    check(
        &[
            ("did", "đi"),
            ("Did", "Đi"),
            ("DiD", "Đi"),
            ("dId", "đI"),
            ("dID", "đI"),
            ("DID", "ĐI"),
            ("dungd", "đung"),
            ("DungD", "Đung"),
            ("duwowngfd", "đường"),
            ("Thienej", "Thiện"),
            ("ThienEj", "Thiện"),
            ("THIENEJ", "THIỆN"),
        ],
        telex,
    );

    // Taking the mark off puts the case the user typed back, so a key that was
    // undone leaves nothing behind. Without this, `Ddd` would end `dd` and the
    // capital the user typed would be gone.
    check(
        &[
            ("Ddd", "Dd"),
            ("dDd", "dd"),
            ("DdD", "DD"),
            ("Aaa", "Aa"),
            ("aAa", "aa"),
            ("aAA", "aA"),
        ],
        telex,
    );
}

/// The three places a modifier key really does decide a case — because in each
/// of them it is not modifying anything, it is the letter.
#[test]
fn a_modifier_that_becomes_a_letter_carries_its_own_case() {
    check(
        &[
            // `w` with no vowel to decorate types `ư` outright.
            ("w", "ư"),
            ("W", "Ư"),
            ("Tw", "Tư"),
            ("tW", "tƯ"),
            ("TW", "TƯ"),
            // The bracket shortcuts, where shift is the whole difference.
            ("]", "ư"),
            ("}", "Ư"),
            ("T]", "Tư"),
            ("T}", "TƯ"),
            ("HU[", "HUơ"),
            ("HU{", "HUƠ"),
            ("THU[R", "THUở"),
            ("THU{R", "THUỞ"),
            // An undo types the key that undid it, in the case it was typed.
            ("caSs", "cas"),
            ("cAsS", "cAS"),
            ("aWw", "aw"),
            ("AwW", "AW"),
            ("dDd", "dd"),
            ("DdD", "DD"),
            ("Caaa", "Caa"),
            ("CAAA", "CAA"),
        ],
        telex,
    );
    // VNI's undo types a digit, which ends the syllable rather than joining it.
    check(&[("A66", "A6"), ("D99", "D9"), ("U77", "U7")], vni);
}

// ---------------------------------------------- boundaries and other keys

#[test]
fn a_space_commits_the_syllable_and_still_reaches_the_application() {
    check(
        &[
            ("tieengs Vieetj", "tiếng Việt"),
            ("xin chaof", "xin chào"),
            ("camr own", "cảm ơn"),
            ("  ", "  "),
        ],
        telex,
    );
}

#[test]
fn punctuation_and_digits_end_a_syllable_without_being_eaten() {
    check(
        &[
            ("chaof!", "chào!"),
            ("mootj, hai", "một, hai"),
            ("nawm 2026", "năm 2026"),
            ("(tieengs)", "(tiếng)"),
            ("a.b.c", "a.b.c"),
            ("1234567890", "1234567890"),
        ],
        telex,
    );
    // In VNI a digit is a diacritic key only where one could land; elsewhere it
    // is the digit.
    check(&[("na8m 2026", "năm 2026"), ("iphone 7", "iphone 7")], vni);
}

#[test]
fn enter_tab_and_escape_commit_and_pass_through() {
    for key in [Key::Enter, Key::Tab, Key::Escape, Key::ArrowLeft, Key::Home] {
        let mut engine = engine();
        let mut host = Host::new();
        for ch in "tieengs".chars() {
            let event = KeyEvent::character(ch);
            let result = engine.process_key(&event);
            host.apply(&result.actions, event.text);
        }
        press(&mut engine, &mut host, key);
        assert_eq!(host.document, "tiếng", "{key:?} lost the syllable");
        assert!(engine.composition().is_empty());
    }
}

/// A shortcut must reach the application, and must not take the half-typed
/// syllable with it.
#[test]
fn a_command_shortcut_commits_and_is_not_consumed() {
    let mut engine = engine();
    let mut host = Host::new();
    for ch in "tieengs".chars() {
        let event = KeyEvent::character(ch);
        let result = engine.process_key(&event);
        host.apply(&result.actions, event.text);
    }

    let save = KeyEvent::character('s').with_modifiers(Modifiers {
        meta: true,
        ..Modifiers::NONE
    });
    let result = engine.process_key(&save);
    assert!(!result.handled, "the application never saw Cmd+S");
    host.apply(&result.actions, None);
    assert_eq!(host.document, "tiếng");
}

// ----------------------------------------------------- backspace and reset

#[test]
fn backspace_removes_one_visible_character_at_a_time() {
    let mut engine = engine();
    let mut host = Host::new();
    for ch in "tieengs".chars() {
        let event = KeyEvent::character(ch);
        let result = engine.process_key(&event);
        host.apply(&result.actions, event.text);
    }
    assert_eq!(host.visible(), "tiếng");

    for (expected, actions) in [
        ("tiến", composition("tiến")),
        ("tiế", composition("tiế")),
        ("ti", composition("ti")),
        ("t", composition("t")),
        ("", vec![EngineAction::ClearComposition]),
    ] {
        let event = KeyEvent::special(Key::Backspace);
        let result = engine.process_key(&event);
        assert_eq!(result.actions, actions);
        host.apply(&result.actions, event.text);
        assert_eq!(host.visible(), expected);
    }
}

/// Backspacing off the end of a composition is the application's problem, not
/// the engine's.
#[test]
fn backspace_with_nothing_composed_passes_through() {
    let mut engine = engine();
    let result = engine.process_key(&KeyEvent::special(Key::Backspace));
    assert!(!result.handled);
    assert_eq!(result.actions, vec![EngineAction::PassThrough]);
}

#[test]
fn typing_continues_after_a_backspace() {
    let mut engine = engine();
    let mut host = Host::new();
    for ch in "tieen".chars() {
        let event = KeyEvent::character(ch);
        let result = engine.process_key(&event);
        host.apply(&result.actions, event.text);
    }
    press(&mut engine, &mut host, Key::Backspace);
    for ch in "ngs".chars() {
        let event = KeyEvent::character(ch);
        let result = engine.process_key(&event);
        host.apply(&result.actions, event.text);
    }
    assert_eq!(host.visible(), "tiếng");
}

#[test]
fn backspace_clears_deferred_english_evidence() {
    let mut engine = engine();
    let mut host = Host::new();
    for key in "marr".chars() {
        let event = KeyEvent::character(key);
        host.apply(&engine.process_key(&event).actions, event.text);
    }
    press(&mut engine, &mut host, Key::Backspace);
    for key in "nw".chars() {
        let event = KeyEvent::character(key);
        host.apply(&engine.process_key(&event).actions, event.text);
    }
    assert_eq!(host.visible(), "măn");
}

#[test]
fn reset_throws_the_composition_away_without_inserting_it() {
    let mut engine = engine();
    let mut host = Host::new();
    for ch in "tieengs".chars() {
        let event = KeyEvent::character(ch);
        let result = engine.process_key(&event);
        host.apply(&result.actions, event.text);
    }
    let result = engine.reset();
    host.apply(&result.actions, None);
    assert_eq!(host.visible(), "");
    assert!(engine.composition().is_empty());

    // And a reset with nothing in flight asks the host for nothing at all.
    assert_eq!(engine.reset(), crate::core::EngineResult::ignored());
}

#[test]
fn commit_accepts_what_is_in_flight() {
    let mut engine = engine();
    let mut host = Host::new();
    for ch in "tieengs".chars() {
        let event = KeyEvent::character(ch);
        let result = engine.process_key(&event);
        host.apply(&result.actions, event.text);
    }
    let result = engine.commit();
    host.apply(&result.actions, None);
    assert_eq!(host.document, "tiếng");
    assert!(engine.composition().is_empty());
    // A second commit has nothing left to do.
    assert!(engine.commit().actions.is_empty());
}

#[test]
fn a_foreign_precomposed_scalar_commits_and_passes_through() {
    for scalar in ['ư', 'é', '日'] {
        let mut engine = engine();
        let mut host = Host::new();
        for key in "tieengs".chars() {
            let event = KeyEvent::character(key);
            let result = engine.process_key(&event);
            host.apply(&result.actions, event.text);
        }

        let event = KeyEvent::character(scalar);
        let result = engine.process_key(&event);
        assert_eq!(
            result.actions,
            vec![EngineAction::CommitComposition, EngineAction::PassThrough],
            "{scalar:?}"
        );
        assert!(!result.handled, "{scalar:?}");
        host.apply(&result.actions, event.text);
        assert_eq!(host.document, format!("tiếng{scalar}"));
        assert!(engine.composition().is_empty());
    }
}

// -------------------------------------------------- English and nonsense

/// A doubled Telex control is one gesture with one visible result, and that
/// holds whatever the engine later decides the word was.
///
/// The first press transforms, the second takes the transformation back and
/// types the letter — so the letter reaches the document **once**, and neither
/// the immediate rendering nor a later English reconstruction may change that
/// count. `arrow` therefore types `arow`, and a typist who wants the English
/// word spells the doubled `r` out: `arrrow`. See
/// [`the_cancellation_reading_costs_english_words_that_double_a_control`] for the
/// words this reading costs and why they were never really being read.
#[test]
fn doubled_control_letter_english_table() {
    let cases = [
        // Tone and clear-tone keys. A repeat is the supported cancellation
        // (`marr` -> `mar`) and stays one letter however the run ends.
        ("arrow", "arow"),
        ("arrows", "arows"),
        ("narrow", "narow"),
        ("borrow", "borow"),
        ("sorrow", "sorow"),
        ("tomorrow", "tomorow"),
        ("carry", "cary"),
        ("sorry", "sory"),
        ("hurry", "hury"),
        ("worry", "ưory"),
        ("berry", "bery"),
        ("error", "eror"),
        ("mirror", "miror"),
        ("horror", "horor"),
        ("terror", "teror"),
        ("marrow", "marow"),
        ("barrow", "barow"),
        ("harrow", "harow"),
        ("class", "class"),
        ("pass", "pas"),
        ("miss", "mis"),
        ("assess", "asess"),
        ("across", "across"),
        ("address", "address"),
        ("off", "of"),
        ("offer", "ofer"),
        ("coffee", "cofee"),
        ("different", "diferent"),
        ("buffer", "bufer"),
        ("effort", "efort"),
        ("jazz", "jazz"),
        ("buzz", "buzz"),
        ("fizz", "fizz"),
        ("pizza", "pizza"),
        // Stroke and circumflex keys. `see` is also exactly how Telex spells
        // Vietnamese `sê`; those valid readings cannot be guessed as English.
        ("add", "add"),
        ("odd", "odd"),
        ("ladder", "ladder"),
        ("sudden", "sudden"),
        ("daddy", "dady"),
        ("middle", "middle"),
        ("hidden", "hidden"),
        ("wedding", "wedding"),
        ("see", "sê"),
        ("meet", "mêt"),
        ("need", "need"),
        ("book", "book"),
        ("door", "dổ"),
        ("food", "food"),
        ("floor", "floor"),
        ("cheese", "chée"),
        ("agree", "agree"),
        ("aardvark", "aardvark"),
        // Case variants obey the same distinction.
        ("Arrow", "Arow"),
        ("ARROW", "AROW"),
        ("Sorry", "Sory"),
    ];
    check(&cases, telex);

    // VNI uses digits as controls, so every alphabetic word in the same table
    // is literal; doubled English letters never enter its undo path.
    for (keys, _) in cases {
        assert_eq!(vni(keys), keys, "{keys}");
    }

    // Every Telex control family takes the deferred path: only a later control
    // over the now-impossible cancelled reading restores the physical keys —
    // and the reconstruction reads a ledger the cancellation has already been
    // discharged from, so each cancelled key still stands exactly once.
    // `maszzw` keeps both its `z`s because `z` clears a tone rather than
    // repeating itself: nothing there was ever cancelled.
    check(
        &[
            ("marrnw", "marnw"),
            ("massnw", "masnw"),
            ("maffnw", "mafnw"),
            ("maxxnw", "maxnw"),
            ("majjnw", "majnw"),
            ("maszzw", "maszzw"),
            ("daddaw", "dadaw"),
            ("aaand", "aand"),
            ("eeend", "eend"),
            ("ooond", "oond"),
        ],
        telex,
    );

    // The cancellation is visible the moment it happens and the reconstruction
    // leaves it alone — `ar` is never re-read as `arr`.
    let mut arrow = engine();
    assert_eq!(
        action_stream(&mut arrow, "arrow"),
        ["a", "ả", "ar", "aro", "arow"].map(composition)
    );
}

/// **A reverting Telex key reaches the document exactly once.**
///
/// This is the invariant the whole doubled-control family rests on. The first
/// press applies a transformation, the second takes it back and types the
/// letter — one gesture, one letter — and neither the immediate rendering nor
/// the later English reconstruction may change that count. The engine keeps it
/// true by discharging the reverting key from the syllable's ledger the moment
/// the revert types it, so nothing downstream can type it a second time; see
/// `Syllable::spend_reverting_key`.
///
/// The expected spellings below are only what the invariant looks like word by
/// word. The property is asserted directly beside each one: the control letter
/// is typed exactly one time fewer than it was pressed.
#[test]
fn a_reverting_control_key_is_typed_exactly_once() {
    // (keys, the control key doubled in them, what lands in the document)
    let cases = [
        // The three reported cases, exactly.
        ("insstead", 's', "instead"),
        ("exxtra", 'x', "extra"),
        ("merrmaid", 'r', "mermaid"),
        // The five tone keys, at the front of a word, in the middle and at the
        // end. A tone key needs a syllable to put a tone on, so the earliest a
        // revert can happen is right after the first nucleus.
        ("assk", 's', "ask"),
        ("mapss", 's', "maps"),
        ("beff", 'f', "bef"),
        ("beffore", 'f', "before"),
        ("offfice", 'f', "office"),
        ("borrn", 'r', "born"),
        ("carr", 'r', "car"),
        ("taxxi", 'x', "taxi"),
        ("boxx", 'x', "box"),
        ("majjor", 'j', "major"),
        ("banjjo", 'j', "banjo"),
        // The horn/breve key, which is a whole letter of its own before a
        // nucleus and a mark after one.
        ("wwet", 'w', "wet"),
        ("bawwl", 'w', "bawl"),
        ("laww", 'w', "law"),
        // The stroke, which reaches back to the syllable's initial `d`.
        ("dddo", 'd', "ddo"),
        ("doddge", 'd', "dodge"),
        ("dadd", 'd', "dad"),
        ("didd", 'd', "did"),
        // The circumflex, where the control is the *second* press of a vowel,
        // so escaping it takes three.
        ("aaand", 'a', "aand"),
        ("neeed", 'e', "need"),
        ("cooop", 'o', "coop"),
        // Case variants count the same key.
        ("Insstead", 's', "Instead"),
        ("INSSTEAD", 's', "INSTEAD"),
        ("Merrmaid", 'r', "Mermaid"),
        ("MERRMAID", 'r', "MERMAID"),
        ("Exxtra", 'x', "Extra"),
        // And the same accounting where no reconstruction happens at all,
        // because no later control ever proves the run foreign. These are the
        // "swallowed" face of the same rule and not a second site: the
        // reverting key is typed once here too, and the press it cancelled is
        // spent on the diacritic it put up and took down.
        ("pass", 's', "pas"),
        ("miss", 's', "mis"),
        ("off", 'f', "of"),
        ("carry", 'r', "cary"),
        ("sorry", 'r', "sory"),
        ("hurry", 'r', "hury"),
        ("daddy", 'd', "dady"),
    ];

    let spellings: Vec<(&str, &str)> = cases.iter().map(|(keys, _, want)| (*keys, *want)).collect();
    check(&spellings, telex);

    for (keys, control, _) in cases {
        let count = |text: &str| {
            text.chars()
                .filter(|key| key.eq_ignore_ascii_case(&control))
                .count()
        };
        let pressed = count(keys);
        let typed = count(&telex(keys));
        assert_eq!(
            typed,
            pressed - 1,
            "{keys}: pressed {control} {pressed} times, typed it {typed} times"
        );
    }

    // VNI spells its controls with digits, so none of these doubled letters is
    // a control there and every key is its own literal.
    for (keys, _, _) in cases {
        assert_eq!(vni(keys), keys, "{keys}");
    }

    // The revert is discharged where it happens, not where the run is later
    // re-read: `ins` is on screen from the fourth keystroke and the last key
    // may not widen it back to `inss`.
    let mut instead = engine();
    assert_eq!(
        action_stream(&mut instead, "insstead"),
        ["i", "in", "ín", "ins", "inst", "inste", "instea", "instead"].map(composition)
    );
}

/// The English words the cancellation reading costs — a decision, not a bug.
///
/// # The choice
///
/// A doubled Telex control has two readings and the engine must pick one
/// before it knows what the word is:
///
/// - **the cancellation** — two presses are one gesture leaving one letter, so
///   `insstead` is `instead`. Chosen (the captain's call, 2026-08-27).
/// - **the keystrokes** — hand back everything typed, so `arrow` is `arrow`.
///   Rejected: it can only be had by not distrusting the raw record after a
///   revert, which deletes the Telex escape itself — `marr` becomes `marr` and
///   there is then no way to type `mar` at all. See
///   [`a_doubled_control_still_cancels_for_vietnamese`], which pins that.
///
/// # Why no third option exists
///
/// The two readings want different text from the *same* engine state. `effort`
/// and `exxtra` are step-for-step identical, six keys each:
///
/// | press | `effort` | `exxtra` |
/// |---|---|---|
/// | 1 | letter → `e` | letter → `e` |
/// | 2 | tone applied → `è` | tone applied → `ẽ` |
/// | 3 | same key reverts → `ef` | same key reverts → `ex` |
/// | 4 | plain letter → `efo` | plain letter → `ext` |
/// | 5 | tone key falls back, run impossible, reconstruction fires | *identical* |
/// | 6 | literal | *identical* |
///
/// Same revert position, same trigger class, same viability, same trust state.
/// The only difference is which letters — which is a lexicon, and a word list
/// of English exceptions is not on the table. So the engine cannot tell a
/// deliberate escape from a word that genuinely doubles the letter, and it
/// honours the cancellation it already showed the user rather than guessing.
///
/// # The cost, and the way round it
///
/// These words come out one letter short. Reaching the English spelling is the
/// same move as typing any literal Telex control — spell the doubling out,
/// `arrrow` — which
/// [`spelling_a_doubled_control_out_types_both_letters`] holds as a contract.
#[test]
fn the_cancellation_reading_costs_english_words_that_double_a_control() {
    check(
        &[
            ("arrow", "arow"),
            ("narrow", "narow"),
            ("mirror", "miror"),
            ("error", "eror"),
            ("offer", "ofer"),
            ("coffee", "cofee"),
            ("assess", "asess"),
            ("buffer", "bufer"),
            ("effort", "efort"),
            ("different", "diferent"),
        ],
        telex,
    );
    // The `effort`/`exxtra` pair from the doc comment above, side by side: two
    // identical runs, and the reading the engine picked shows in both.
    check(&[("effort", "efort"), ("exxtra", "extra")], telex);
}

/// **The Telex escape, which the cancellation reading exists to preserve.**
///
/// Doubling a control is how a Vietnamese typist takes a diacritic back and
/// gets the letter: `marr` is the supported way to type `mar`, and there is no
/// other way to type it. Every one of these words is what the rejected
/// keystrokes reading would have deleted — under it `marr` is `marr`, `aww` is
/// `aww` and `didd` is `didd`, and the letters below become unreachable.
///
/// So this table is the other half of the decision recorded in
/// [`the_cancellation_reading_costs_english_words_that_double_a_control`]. A
/// later change that makes `arrow` type `arrow` again will fail here, which is
/// the point: that outcome cannot be bought without giving this up.
#[test]
fn a_doubled_control_still_cancels_for_vietnamese() {
    check(
        &[
            // The five tone keys.
            ("marr", "mar"),
            ("mass", "mas"),
            ("maff", "maf"),
            ("maxx", "max"),
            ("majj", "maj"),
            // The horn/breve, the stroke and the circumflex.
            ("aww", "aw"),
            ("oww", "ow"),
            ("uww", "uw"),
            ("didd", "did"),
            ("dadd", "dad"),
            ("aaa", "aa"),
            ("eee", "ee"),
            ("ooo", "oo"),
            // Cancelling one control leaves every other Telex rule intact, so
            // the syllable is still composed around it.
            ("toanr", "toản"),
        ],
        telex,
    );
}

/// Spelling the doubling out reaches a real double letter.
///
/// This is the documented way round the cost recorded in
/// [`the_cancellation_reading_costs_english_words_that_double_a_control`], so
/// it is a contract rather than folklore: the third press has no
/// transformation left to revert, falls back to its own literal, and the run
/// goes literal from there.
#[test]
fn spelling_a_doubled_control_out_types_both_letters() {
    check(
        &[
            ("arrrow", "arrow"),
            ("narrrow", "narrow"),
            ("errror", "error"),
            ("offfer", "offer"),
            ("cofffee", "coffee"),
            ("asssess", "assess"),
            ("bufffer", "buffer"),
            ("efffort", "effort"),
            ("diffferent", "different"),
            ("passs", "pass"),
            ("misss", "miss"),
            ("offf", "off"),
            ("carrry", "carry"),
        ],
        telex,
    );
}

/// A word-final `w` remains part of an English word once the trustworthy run
/// has proved non-Vietnamese. Real Telex/VNI marks stay available in syllables
/// where their key or digit has a target.
#[test]
fn english_words_ending_in_w_stay_literal_without_costing_real_marks() {
    let english = &[
        ("window", "window"),
        ("gateway", "gateway"),
        ("follow", "follow"),
        ("widow", "widow"),
        ("willow", "willow"),
        ("shadow", "shadow"),
        ("below", "below"),
        ("elbow", "elbow"),
    ];
    check(english, telex);
    check(english, vni);
    // The `-rrow` words reach the final `w` through a cancelled `r`, so their
    // Telex spelling is one `r` short of the English one — see
    // `the_cancellation_reading_costs_english_words_that_double_a_control`. VNI has
    // no alphabetic control at all and leaves them whole.
    let rrow = &[
        ("arrow", "arow"),
        ("narrow", "narow"),
        ("borrow", "borow"),
        ("sorrow", "sorow"),
        ("tomorrow", "tomorow"),
        ("marrow", "marow"),
    ];
    check(rrow, telex);
    for (keys, _) in rrow {
        assert_eq!(vni(keys), *keys, "{keys}");
    }
    check(&[("mow", "mơ"), ("tuw", "tư"), ("bawng", "băng")], telex);
    check(&[("mo7", "mơ"), ("tu7", "tư"), ("ba8ng", "băng")], vni);
}

/// The spell-check fallback: a syllable that is not Vietnamese is handed back
/// as the keys that were typed.
#[test]
fn non_vietnamese_words_fall_through_as_typed() {
    check(
        &[
            ("hello", "hello"),
            ("world", "world"),
            ("sport", "sport"),
            ("where", "where"),
            ("switch", "switch"),
            ("string", "string"),
            ("zebra", "zebra"),
            ("json", "json"),
            ("http", "http"),
            ("stack", "stack"),
            ("crash", "crash"),
            // A `w` after a nucleus is not automatically a standalone `ư`.
            ("new", "new"),
            ("view", "view"),
            ("few", "few"),
            // An impossible onset disables later Telex controls for this run.
            ("window", "window"),
            ("framework", "framework"),
            ("software", "software"),
            ("browser", "browser"),
            ("NEW", "NEW"),
            ("View", "View"),
            ("Browser", "Browser"),
            // A cancelled doubled control makes the run literal only when a
            // later control supplies the missing evidence, and the cancelled
            // key is not typed again when it does.
            ("arrow", "arow"),
            ("assess", "asess"),
            ("offer", "ofer"),
            ("coffee", "cofee"),
            ("error", "eror"),
            ("Arrow", "Arow"),
            ("ARROW", "AROW"),
        ],
        telex,
    );

    let mut new = engine();
    assert_eq!(
        action_stream(&mut new, "new"),
        ["n", "ne", "new"].map(composition)
    );

    let mut software = engine();
    assert_eq!(
        action_stream(&mut software, "software"),
        [
            "s", "so", "sò", "soft", "softw", "softwa", "softwar", "software"
        ]
        .map(composition)
    );

    check(&[("framework dduwowcj", "framework được")], telex);
}

/// A checked Vietnamese syllable ending in a stop coda can carry only sắc or
/// nặng. Other tone keys remain literal, whichever side of the coda they were
/// typed on.
#[test]
fn stop_codas_reject_incompatible_tones() {
    check(
        &[
            ("cats", "cát"),
            ("catj", "cạt"),
            ("catf", "catf"),
            ("catr", "catr"),
            ("catx", "catx"),
            ("cas t", "cá t"),
            ("cast", "cát"),
            ("caft", "caft"),
        ],
        telex,
    );
}

#[test]
fn required_normal_words_still_converge() {
    check(
        &[
            ("dduwowcj", "được"),
            ("thuwowng", "thương"),
            ("thuowng", "thương"),
            ("nguwowif", "người"),
            ("nuwowcs", "nước"),
            ("truwowngf", "trường"),
            ("tuwowngr", "tưởng"),
            ("huwowngr", "hưởng"),
            ("THUOW", "THUƠ"),
            ("THUOWN", "THƯƠN"),
            ("Thuowngf", "Thường"),
        ],
        telex,
    );
}

/// The honest limitation, written down as a test so nobody "fixes" it by
/// accident: an English word whose Telex reading *is* a Vietnamese syllable
/// gets transformed, exactly as it does in Unikey.
#[test]
fn an_english_word_that_reads_as_vietnamese_is_transformed() {
    check(
        &[
            ("test", "tét"),
            ("cats", "cát"),
            ("man", "man"),
            // `w` is `ư`, and `ưong` has the shape of a syllable even though no
            // Vietnamese word has that nucleus. The structural check in
            // `rules` is deliberately not a word list; see its module docs.
            ("wrong", "ửong"),
        ],
        telex,
    );
}

/// With the setting off, the rendered syllable always stands.
#[test]
fn spell_check_can_be_turned_off() {
    let mut off = configured(VietnameseConfig {
        spell_check: false,
        ..VietnameseConfig::default()
    });
    assert_eq!(type_keys(&mut off, "where"), "ưhere");
    let mut off = configured(VietnameseConfig {
        spell_check: false,
        ..VietnameseConfig::default()
    });
    assert_eq!(type_keys(&mut off, "hello"), "hello");
}

#[test]
fn mixed_english_and_vietnamese_keeps_the_boundaries() {
    check(
        &[
            ("Hello, tooi laf An", "Hello, tôi là An"),
            ("email: an@vidu.vn", "email: an@vidu.vn"),
            ("gitlab CI chayj oki", "gitlab CI chạy oki"),
            ("HTTP 404 khoong timf thaasy", "HTTP 404 không tìm thấy"),
        ],
        telex,
    );
}

#[test]
fn a_key_the_engine_cannot_use_is_never_swallowed() {
    // Every printable ASCII character, typed alone, arrives somewhere.
    for code in 0x20u8..0x7f {
        let ch = code as char;
        let typed = telex(&ch.to_string());
        assert!(!typed.is_empty(), "{ch:?} vanished",);
    }
}

// ------------------------------------------------------------ brackets

/// The explicit shortcut for `uơ`.
#[test]
fn the_bracket_shortcuts_type_o_horn_and_u_horn() {
    check(&[("thu[r", "thuở"), ("hu[", "huơ"), ("t]", "tư")], telex);

    let mut off = configured(VietnameseConfig {
        bracket_shortcuts: false,
        ..VietnameseConfig::default()
    });
    assert_eq!(type_keys(&mut off, "thu["), "thu[");
}

// ------------------------------------------------------ the output modes

/// Direct output types for real and rewrites what it typed, and must arrive at
/// exactly the same text.
#[test]
fn direct_output_reaches_the_same_words() {
    for word in corpus::WORDS.iter().take(80) {
        let keys = corpus::telex_keys(word);
        let mut engine = configured(VietnameseConfig {
            output: OutputMode::Direct,
            ..VietnameseConfig::default()
        });
        assert_eq!(type_keys(&mut engine, &keys), *word, "{keys}");
    }
}

#[test]
fn direct_output_uses_the_replacement_actions() {
    let mut engine = configured(VietnameseConfig {
        output: OutputMode::Direct,
        ..VietnameseConfig::default()
    });

    let first = engine.process_key(&KeyEvent::character('t'));
    assert_eq!(first.actions, vec![EngineAction::InsertText("t".into())]);

    let second = engine.process_key(&KeyEvent::character('i'));
    assert_eq!(
        second.actions,
        vec![EngineAction::ReplaceBeforeCursor {
            grapheme_count: 1,
            text: "ti".into(),
        }]
    );

    // And a backspace back to nothing is a deletion.
    let mut engine = configured(VietnameseConfig {
        output: OutputMode::Direct,
        ..VietnameseConfig::default()
    });
    engine.process_key(&KeyEvent::character('t'));
    let deleted = engine.process_key(&KeyEvent::special(Key::Backspace));
    assert_eq!(deleted.actions, vec![EngineAction::DeleteBackward(1)]);
}

#[test]
fn composition_output_marks_text_and_commits_it() {
    let mut engine = engine();
    let first = engine.process_key(&KeyEvent::character('t'));
    assert_eq!(
        first.actions,
        vec![EngineAction::SetComposition {
            text: "t".into(),
            cursor: 1,
            selection: None,
        }]
    );
    assert_eq!(engine.composition().text(), "t");

    let space = engine.process_key(&KeyEvent::character(' '));
    assert_eq!(
        space.actions,
        vec![EngineAction::CommitComposition, EngineAction::PassThrough]
    );
    assert!(!space.handled);
}

/// The syllable is finished under the rules it was typed with.
#[test]
fn changing_the_configuration_commits_first() {
    let mut engine = engine();
    let mut host = Host::new();
    for ch in "tieengs".chars() {
        let event = KeyEvent::character(ch);
        let result = engine.process_key(&event);
        host.apply(&result.actions, event.text);
    }
    let result = engine.set_config(VietnameseConfig {
        scheme: InputScheme::Vni,
        ..VietnameseConfig::default()
    });
    host.apply(&result.actions, None);
    assert_eq!(host.document, "tiếng");
    assert_eq!(engine.config().scheme, InputScheme::Vni);
}

// --------------------------------------------------------------- the rest

#[test]
fn the_engine_names_itself() {
    assert_eq!(engine().language(), LanguageId::Vietnamese);
    assert_eq!(engine().config(), VietnameseConfig::default());
    assert_eq!(VietnameseConfig::default().scheme, InputScheme::Telex);
    assert_eq!(
        VietnameseConfig::default().tone_placement,
        TonePlacement::Modern
    );
}

#[test]
fn a_composition_is_visible_before_it_is_committed() {
    let mut engine = engine();
    assert_eq!(type_keys_uncommitted(&mut engine, "tieeng"), "tiêng");
    assert_eq!(engine.composition().text(), "tiêng");
    assert_eq!(engine.composition().cursor(), 5);
}

#[test]
fn everything_this_engine_emits_is_nfc() {
    for keys in [
        "tieengs",
        "dduwowngf",
        "VIEETJ",
        "nguyeenx",
        "hoaf khoer thuyr",
        "hello world",
    ] {
        let typed = telex(keys);
        assert_eq!(nfc(&typed), typed, "{keys}");
    }
}

/// # The cost of a keystroke
///
/// Processing one key is: one `interpret` (a few `match`es over a syllable that
/// is at most seven letters), one mutation of a `Vec<Letter>` that never
/// reallocates past its first growth, and one `render` — which walks those
/// letters, does one table lookup and one NFC pass each, and allocates one
/// small `String`. There is no search, no backtracking, no dictionary, no
/// regular expression and no allocation proportional to anything but the
/// syllable. The state machine's work is bounded by the length of a Vietnamese
/// syllable, which is bounded by the language.
///
/// The measurement is generous on purpose: the real figure on a developer
/// machine is on the order of a microsecond per key, and the bound below is
/// three orders of magnitude looser so that a loaded CI runner cannot turn a
/// timing observation into a red build. It is here to catch a change that makes
/// this accidentally quadratic, not to defend a number.
#[test]
fn the_normal_path_is_cheap() {
    let sequences: Vec<String> = corpus::WORDS
        .iter()
        .map(|word| corpus::telex_keys(word))
        .collect();
    let keys: usize = sequences.iter().map(|keys| keys.chars().count()).sum();

    let started = std::time::Instant::now();
    let rounds = 20;
    for _ in 0..rounds {
        for sequence in &sequences {
            let mut engine = engine();
            let _ = type_keys(&mut engine, sequence);
        }
    }
    let elapsed = started.elapsed();

    let total = keys * rounds;
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "{total} keystrokes took {elapsed:?}; something has become superlinear"
    );
}
