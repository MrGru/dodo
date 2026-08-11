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
    let mut telex = engine();
    assert_eq!(
        action_stream(&mut telex, "thuwowng"),
        ["t", "th", "thu", "thư", "thưo", "thươ", "thươn", "thương"].map(composition)
    );

    let mut incremental = engine();
    assert_eq!(
        action_stream(&mut incremental, "thuow"),
        ["t", "th", "thu", "thuo", "thươ"].map(composition)
    );
    assert_eq!(action_stream(&mut incremental, "w"), [composition("thưow")]);
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
            ("caf f", "cà f"),
            ("cassa", "casa"),
            ("aww", "aw"),
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

/// The VNI half of the same rule. A digit cannot join a syllable, so the
/// syllable ends and the digit is typed by the application: `a11` is `a1`.
/// Unlike Telex's bare `w`, no VNI digit creates a whole marked letter, so
/// `u77` still retains its physical `u`.
#[test]
fn a_repeated_vni_digit_undoes_itself_and_types_its_digit() {
    check(
        &[
            ("a66", "a6"),
            ("a11", "a1"),
            ("d99", "d9"),
            ("u77", "u7"),
            ("a88", "a8"),
            ("ma11n", "ma1n"),
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
/// **A modifier key decides which diacritic; the letter it lands on decides
/// the case.** `Aa`, `aA` and `AA` are three ways of typing a circumflex onto
/// an `a` that was already typed uppercase, lowercase, uppercase — so they give
/// `Â`, `â`, `Â`. Shift is how a typist reaches `S` for *sắc* on a caps-locked
/// word; it is not an opinion about the letter underneath, and Vietnamese has
/// no convention in which it could be one. VNI is the same rule stated more
/// obviously, because a digit has no case at all to leak.
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
            // The doubled vowels: the first `a` is the letter, the second is
            // only a circumflex.
            ("aa", "â"),
            ("aA", "â"),
            ("Aa", "Â"),
            ("AA", "Â"),
            ("ee", "ê"),
            ("eE", "ê"),
            ("Ee", "Ê"),
            ("EE", "Ê"),
            ("oo", "ô"),
            ("oO", "ô"),
            ("Oo", "Ô"),
            ("OO", "Ô"),
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
            // `dd`: the stroke belongs to the leading `d`, whichever `d` was
            // shifted.
            ("dd", "đ"),
            ("dD", "đ"),
            ("Dd", "Đ"),
            ("DD", "Đ"),
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

    // Mixed case inside one syllable is where a leaked modifier case would
    // show: every letter below keeps exactly the case it was typed with.
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

/// The only way to reach `uơ`.
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
