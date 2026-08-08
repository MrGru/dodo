//! A few hundred real Vietnamese words and phrases, and the round trip that
//! turns them into a test.
//!
//! # Why the key sequences are generated
//!
//! Writing `("tieengs", "tiếng")` three hundred times by hand would test
//! whatever the author believed while typing it. Instead [`WORDS`] holds only
//! the **answers** — real Vietnamese, spelled correctly — and [`telex_keys`]
//! and [`vni_keys`] derive the keystrokes by decomposing each word.
//!
//! That is not circular, because the generators know nothing the engine has to
//! get right. They spell a diacritic (`ê` is `ee` in Telex, `e6` in VNI) and
//! append the tone key at the **end of the syllable**, wherever the tone mark
//! actually sits. Everything the engine is being tested on — which vowel takes
//! the tone, how `qu` and `gi` absorb their glide, how the tone moves when a
//! final consonant arrives, whether `ươ` gets two horns — is absent from the
//! generators and must be re-derived by the state machine.
//!
//! The five worked examples in the specification fall straight out of it:
//! `telex_keys("tiếng")` is `tieengs`, `telex_keys("đường")` is `dduwowngf`,
//! `telex_keys("Việt")` is `Vieetj`, `telex_keys("đăng")` is `ddawng`,
//! `telex_keys("Nguyễn")` is `Nguyeenx`. They are asserted as such in
//! [`tests`](super::tests), against a hand-written table, so a broken generator
//! cannot quietly agree with a broken engine.
//!
//! # Two things the list deliberately avoids
//!
//! - **Open `oa`, `oe` and `uy` are spelled the modern way** — `hoà`, `khoẻ`,
//!   `thuỷ`, `Thanh Hoá` — because that is the engine's default. The
//!   traditional spellings are tested separately, against
//!   [`TonePlacement::Traditional`](super::TonePlacement::Traditional).
//! - **No `uơ` and no `oo`.** `thuở` and `xoong` cannot be typed with `w` or
//!   with a doubled `o` — see [`super::telex`] for why, and for the bracket
//!   shortcut that reaches `uơ`. They are covered there rather than being
//!   smuggled in here where the round trip would fail for a reason that is not
//!   a bug.

use super::syllable::{Mark, Tone};

/// Real Vietnamese, correctly spelled: the answers the round trip must
/// reproduce from generated keystrokes, in both Telex and VNI.
///
/// Ordinary vocabulary, place names, family names and short phrases, chosen to
/// cover every initial cluster, every final cluster, every nucleus that occurs
/// with any frequency, all six tones, and capitalization at the start of a word
/// and throughout.
pub const WORDS: [&str; 461] = [
    // People and family.
    "tôi",
    "bạn",
    "anh",
    "chị",
    "em",
    "ông",
    "bà",
    "cô",
    "chú",
    "bác",
    "cháu",
    "con",
    "mẹ",
    "cha",
    "bố",
    "má",
    "vợ",
    "chồng",
    "gia đình",
    "họ hàng",
    "người lớn",
    "trẻ em",
    "đàn ông",
    "phụ nữ",
    "con trai",
    "con gái",
    "bạn bè",
    "hàng xóm",
    // Numbers.
    "một",
    "hai",
    "ba",
    "bốn",
    "năm",
    "sáu",
    "bảy",
    "tám",
    "chín",
    "mười",
    "trăm",
    "nghìn",
    "triệu",
    "tỷ",
    "mười một",
    "hai mươi",
    "một nghìn",
    // Time.
    "giờ",
    "phút",
    "giây",
    "ngày",
    "tuần",
    "tháng",
    "hôm nay",
    "hôm qua",
    "ngày mai",
    "sáng",
    "trưa",
    "chiều",
    "tối",
    "đêm",
    "mùa xuân",
    "mùa hè",
    "mùa thu",
    "mùa đông",
    "bây giờ",
    "lúc nãy",
    "sau này",
    "quá khứ",
    "hiện tại",
    "tương lai",
    // Verbs.
    "ăn",
    "uống",
    "ngủ",
    "đi",
    "đến",
    "về",
    "làm",
    "học",
    "đọc",
    "viết",
    "nói",
    "nghe",
    "xem",
    "nhìn",
    "thấy",
    "biết",
    "hiểu",
    "nghĩ",
    "nhớ",
    "quên",
    "yêu",
    "thích",
    "ghét",
    "mua",
    "bán",
    "cho",
    "nhận",
    "gửi",
    "tìm",
    "mở",
    "đóng",
    "chạy",
    "nhảy",
    "đứng",
    "ngồi",
    "nằm",
    "cười",
    "khóc",
    "hỏi",
    "giúp",
    "bắt đầu",
    "kết thúc",
    "trả lời",
    "làm việc",
    "nghỉ ngơi",
    "cố gắng",
    "chờ đợi",
    "gặp gỡ",
    // Things and places.
    "nhà",
    "cửa",
    "phòng",
    "bàn",
    "ghế",
    "giường",
    "sách",
    "vở",
    "bút",
    "giấy",
    "máy tính",
    "điện thoại",
    "xe",
    "đường",
    "phố",
    "chợ",
    "trường",
    "lớp",
    "bệnh viện",
    "ngân hàng",
    "công ty",
    "thành phố",
    "quốc gia",
    "thế giới",
    "sân bay",
    "nhà ga",
    "khách sạn",
    "quán ăn",
    "công viên",
    "thư viện",
    // Food.
    "nước",
    "cơm",
    "phở",
    "bánh mì",
    "cà phê",
    "trà",
    "muối",
    "thịt",
    "cá",
    "rau",
    "trái cây",
    "xôi",
    "chè",
    "kem",
    "bún chả",
    "chả giò",
    "cơm tấm",
    "nem rán",
    "canh chua",
    "gỏi cuốn",
    "cháo gà",
    "mì quảng",
    // Nature.
    "hoa",
    "cây",
    "lá",
    "trời",
    "mây",
    "mưa",
    "nắng",
    "gió",
    "biển",
    "núi",
    "sông",
    "rừng",
    "đất",
    "lửa",
    "trăng",
    "sao",
    "chim",
    "cá voi",
    "con mèo",
    "con chó",
    // Qualities.
    "tốt",
    "xấu",
    "đẹp",
    "lớn",
    "nhỏ",
    "cao",
    "thấp",
    "dài",
    "ngắn",
    "rộng",
    "hẹp",
    "nhanh",
    "chậm",
    "nóng",
    "lạnh",
    "ấm",
    "mát",
    "mới",
    "cũ",
    "trẻ",
    "già",
    "khoẻ",
    "mệt",
    "vui",
    "buồn",
    "giàu",
    "nghèo",
    "dễ",
    "khó",
    "đúng",
    "sai",
    "sạch",
    "bẩn",
    "ngon",
    "xa",
    "gần",
    "nhiều",
    "ít",
    "đầy",
    "vắng",
    // Colours.
    "xanh",
    "đỏ",
    "vàng",
    "trắng",
    "đen",
    "tím",
    "nâu",
    "hồng",
    "xám",
    "xanh lá",
    // Grammar words, which are where the odd short syllables live.
    "của",
    "với",
    "từ",
    "trong",
    "ngoài",
    "trên",
    "dưới",
    "trước",
    "sau",
    "giữa",
    "bên",
    "và",
    "hoặc",
    "nhưng",
    "vì",
    "nên",
    "để",
    "nếu",
    "thì",
    "mà",
    "rất",
    "cũng",
    "vẫn",
    "chưa",
    "đã",
    "sẽ",
    "đang",
    "được",
    "phải",
    "cần",
    "này",
    "kia",
    "đó",
    "ai",
    "gì",
    "đâu",
    "nào",
    "mấy",
    "bao nhiêu",
    // Work and study.
    "học sinh",
    "sinh viên",
    "giáo viên",
    "bác sĩ",
    "y tá",
    "kỹ sư",
    "công nhân",
    "nông dân",
    "nghệ sĩ",
    "ca sĩ",
    "nhà văn",
    "nhà báo",
    "thợ may",
    "đầu bếp",
    "lịch sử",
    "văn hoá",
    "kinh tế",
    "khoa học",
    "giáo dục",
    "y tế",
    "thể thao",
    "âm nhạc",
    "hội hoạ",
    "điện ảnh",
    "xã hội",
    "chính phủ",
    "nhân dân",
    "nghiên cứu",
    // The `ươ` family, which is where two horns have to land correctly.
    "người",
    "thường",
    "sương",
    "gương",
    "xương",
    "cương",
    "bướm",
    "vượt",
    "bước",
    "lượng",
    "rượu",
    "hươu",
    "ướt",
    "ước",
    "mượn",
    // The `uô` and `uy` families.
    "chuối",
    "chuột",
    "muỗi",
    "ruồi",
    "đuôi",
    "cuối",
    "suối",
    "buổi",
    "tuổi",
    "thuyền",
    "tuyệt",
    "tuyển",
    "khuyên",
    "khuyến",
    "chuyên",
    "chuyện",
    "quyển",
    "nguyên",
    "nguyện",
    "thuỷ",
    "tuỳ",
    "suy",
    "huy",
    "khuya",
    "khuỷu",
    "nguy",
    // `qu` and `gi`, where a glide belongs to the initial.
    "quả",
    "quán",
    "quen",
    "quê",
    "quốc",
    "quý",
    "quyền",
    "quyết",
    "quang",
    "quảng",
    "quân",
    "quận",
    "quà",
    "quay",
    "quai",
    "quen thuộc",
    "giữ",
    "giếng",
    "giành",
    "giải",
    "giới",
    "giá",
    "giả",
    "giòn",
    "giếng nước",
    // `ngh`, `ng`, `nh`, `tr`, `th`, `kh`, `ph`, `ch`, `gh`.
    "nghỉ",
    "nghiêm",
    "nghiêng",
    "ngành",
    "ngọt",
    "ngựa",
    "ngôi",
    "ngoại",
    "nguồn",
    "nguy hiểm",
    "nhanh nhẹn",
    "nhỏ nhắn",
    "trong trẻo",
    "thẳng thắn",
    "không khí",
    "phong phú",
    "chăm chỉ",
    "ghi chép",
    // Places.
    "Việt Nam",
    "Hà Nội",
    "Sài Gòn",
    "Huế",
    "Đà Nẵng",
    "Hải Phòng",
    "Cần Thơ",
    "Nha Trang",
    "Vũng Tàu",
    "Quảng Ninh",
    "Nghệ An",
    "Thanh Hoá",
    "Bình Dương",
    "Đồng Nai",
    "Tây Ninh",
    "Lâm Đồng",
    "Phú Quốc",
    "Hạ Long",
    "Hội An",
    // Family names.
    "Nguyễn",
    "Trần",
    "Lê",
    "Phạm",
    "Hoàng",
    "Huỳnh",
    "Phan",
    "Vũ",
    "Võ",
    "Đặng",
    "Bùi",
    "Đỗ",
    "Hồ",
    "Ngô",
    "Dương",
    "Lý",
    "Trịnh",
    "Đinh",
    // Phrases.
    "xin chào",
    "cảm ơn",
    "xin lỗi",
    "tạm biệt",
    "chúc mừng",
    "không sao",
    "được rồi",
    "rất vui",
    "hẹn gặp lại",
    "chúc ngủ ngon",
    "tiếng Việt",
    "tôi không hiểu",
    "bạn khoẻ không",
    "tôi tên là",
    "rất hân hạnh",
    "hạnh phúc",
    "may mắn",
    "thành công",
    "hy vọng",
    "ước mơ",
    "yêu thương",
    // Capitalization.
    "VIỆT",
    "TIẾNG VIỆT",
    "ĐƯỜNG",
    "NGUYỄN",
    "HÀ NỘI",
    "CẢM ƠN",
    "ĐĂNG",
    "Xin Chào",
    "Tiếng Việt",
    "Cảm Ơn",
    "Đường",
    "Người",
    "Quốc",
];

/// A word taken apart into the facts the input schemes spell.
struct Decomposed {
    base: char,
    mark: Option<Mark>,
    tone: Tone,
    upper: bool,
}

/// One character of correctly-spelled Vietnamese, taken apart.
///
/// Returns `None` for anything that is not a Vietnamese letter — a space, a
/// comma — which the generators pass through unchanged.
fn decompose(ch: char) -> Option<Decomposed> {
    let upper = ch.is_uppercase();
    let lower = ch.to_lowercase().next().unwrap_or(ch);
    // `đ` has no canonical decomposition; every other Vietnamese letter does.
    if lower == 'đ' {
        return Some(Decomposed {
            base: 'd',
            mark: Some(Mark::Stroke),
            tone: Tone::Level,
            upper,
        });
    }

    let mut chars = unicode_normalization::UnicodeNormalization::nfd(lower);
    let base = chars.next()?;
    if !base.is_ascii_alphabetic() {
        return None;
    }
    let mut mark = None;
    let mut tone = Tone::Level;
    for combining in chars {
        match combining {
            '\u{0302}' => mark = Some(Mark::Circumflex),
            '\u{0306}' => mark = Some(Mark::Breve),
            '\u{031b}' => mark = Some(Mark::Horn),
            '\u{0301}' => tone = Tone::Acute,
            '\u{0300}' => tone = Tone::Grave,
            '\u{0309}' => tone = Tone::HookAbove,
            '\u{0303}' => tone = Tone::Tilde,
            '\u{0323}' => tone = Tone::UnderDot,
            _ => return None,
        }
    }
    Some(Decomposed {
        base,
        mark,
        tone,
        upper,
    })
}

fn cased(ch: char, upper: bool) -> char {
    if upper { ch.to_ascii_uppercase() } else { ch }
}

/// The Telex keystrokes that spell `word`.
///
/// A diacritic doubles its vowel (`ee`) or follows it with `w` (`ow`); `đ` is
/// `dd`; the tone key goes at the end of the syllable, in the case of the
/// letter that carries the mark.
pub fn telex_keys(word: &str) -> String {
    keys(word, |out, letter| {
        out.push(cased(letter.base, letter.upper));
        match letter.mark {
            Some(Mark::Circumflex) => out.push(cased(letter.base, letter.upper)),
            Some(Mark::Breve) | Some(Mark::Horn) => out.push(cased('w', letter.upper)),
            Some(Mark::Stroke) => out.push(cased('d', letter.upper)),
            None => {}
        }
        match letter.tone {
            Tone::Level => None,
            Tone::Acute => Some(cased('s', letter.upper)),
            Tone::Grave => Some(cased('f', letter.upper)),
            Tone::HookAbove => Some(cased('r', letter.upper)),
            Tone::Tilde => Some(cased('x', letter.upper)),
            Tone::UnderDot => Some(cased('j', letter.upper)),
        }
    })
}

/// The VNI keystrokes that spell `word`.
///
/// A diacritic follows its vowel with `6`/`7`/`8`, `đ` is `d9`, and the tone
/// digit goes at the end of the syllable.
pub fn vni_keys(word: &str) -> String {
    keys(word, |out, letter| {
        out.push(cased(letter.base, letter.upper));
        match letter.mark {
            Some(Mark::Circumflex) => out.push('6'),
            Some(Mark::Horn) => out.push('7'),
            Some(Mark::Breve) => out.push('8'),
            Some(Mark::Stroke) => out.push('9'),
            None => {}
        }
        match letter.tone {
            Tone::Level => None,
            Tone::Acute => Some('1'),
            Tone::Grave => Some('2'),
            Tone::HookAbove => Some('3'),
            Tone::Tilde => Some('4'),
            Tone::UnderDot => Some('5'),
        }
    })
}

/// The shared walk: spell each letter, hold the tone key back until the
/// syllable ends.
///
/// Holding the tone key to the end is the point. The engine has to work out for
/// itself *which vowel* it belongs on; the generator only knows *that* the
/// syllable has one.
fn keys(word: &str, mut spell: impl FnMut(&mut String, &Decomposed) -> Option<char>) -> String {
    let mut out = String::with_capacity(word.len() * 2);
    let mut pending_tone: Option<char> = None;

    for ch in word.chars() {
        match decompose(ch) {
            Some(letter) => {
                if let Some(key) = spell(&mut out, &letter) {
                    pending_tone = Some(key);
                }
            }
            None => {
                // A space or a comma ends the syllable, so the tone key has to
                // be typed before it.
                if let Some(key) = pending_tone.take() {
                    out.push(key);
                }
                out.push(ch);
            }
        }
    }
    if let Some(key) = pending_tone.take() {
        out.push(key);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{WORDS, telex_keys, vni_keys};
    use crate::core::grapheme_count;
    use crate::languages::vietnamese::unicode::nfc;
    use crate::languages::vietnamese::{InputScheme, VietnameseConfig, VietnameseEngine};
    use crate::testing::type_keys;

    fn engine(scheme: InputScheme) -> VietnameseEngine {
        VietnameseEngine::new(VietnameseConfig {
            scheme,
            ..VietnameseConfig::default()
        })
    }

    /// The whole corpus, typed in Telex.
    #[test]
    fn every_word_survives_a_telex_round_trip() {
        let mut failures: Vec<String> = Vec::new();
        for word in WORDS {
            let keys = telex_keys(word);
            let typed = type_keys(&mut engine(InputScheme::Telex), &keys);
            if typed != *word {
                failures.push(format!("{keys} -> {typed:?}, wanted {word:?}"));
            }
        }
        assert!(
            failures.is_empty(),
            "{} of {} words failed in Telex:\n  {}",
            failures.len(),
            WORDS.len(),
            failures.join("\n  ")
        );
    }

    /// The identical corpus, typed in VNI. Same answers, different keyboard —
    /// which is the whole claim that the two schemes share one engine.
    #[test]
    fn every_word_survives_a_vni_round_trip() {
        let mut failures: Vec<String> = Vec::new();
        for word in WORDS {
            let keys = vni_keys(word);
            let typed = type_keys(&mut engine(InputScheme::Vni), &keys);
            if typed != *word {
                failures.push(format!("{keys} -> {typed:?}, wanted {word:?}"));
            }
        }
        assert!(
            failures.is_empty(),
            "{} of {} words failed in VNI:\n  {}",
            failures.len(),
            WORDS.len(),
            failures.join("\n  ")
        );
    }

    /// Stated separately from the two round trips above, because "Telex and VNI
    /// agree" is a different claim from "each is correct" — and a shared bug
    /// would satisfy this one alone.
    #[test]
    fn telex_and_vni_agree_on_every_word() {
        for word in WORDS {
            let by_telex = type_keys(&mut engine(InputScheme::Telex), &telex_keys(word));
            let by_vni = type_keys(&mut engine(InputScheme::Vni), &vni_keys(word));
            assert_eq!(by_telex, by_vni, "{word}");
        }
    }

    /// Nothing leaves this engine in NFD, ever.
    #[test]
    fn every_output_is_nfc() {
        for word in WORDS {
            let typed = type_keys(&mut engine(InputScheme::Telex), &telex_keys(word));
            assert_eq!(nfc(&typed), typed, "{word} is not NFC");
            // And the corpus itself is NFC, or the comparison above proves
            // nothing.
            assert_eq!(nfc(word), *word, "{word} in the corpus is not NFC");
        }
    }

    /// A syllable is as many visible characters as it has letters, whatever the
    /// diacritics.
    #[test]
    fn diacritics_never_change_how_long_a_word_looks() {
        for word in WORDS {
            let keys = telex_keys(word);
            let letters = keys.chars().count();
            // Every diacritic and every tone costs a key and adds no visible
            // character, so the rendered word is never longer than the keys
            // that typed it.
            assert!(
                grapheme_count(word) <= letters,
                "{word} is longer than the {letters} keys that typed it"
            );
        }
    }

    #[test]
    fn the_corpus_is_the_size_it_claims_and_holds_no_duplicates() {
        let mut sorted = WORDS.to_vec();
        sorted.sort_unstable();
        let count = sorted.len();
        sorted.dedup();
        assert_eq!(sorted.len(), count, "the corpus repeats itself");
        assert!(count >= 300, "the corpus has shrunk to {count} entries");
    }

    /// The generators are not allowed to be clever. If one of these drifts, the
    /// round trips above would still pass while testing something else.
    #[test]
    fn the_generators_produce_the_specifications_worked_examples() {
        assert_eq!(telex_keys("tiếng"), "tieengs");
        assert_eq!(telex_keys("Việt"), "Vieetj");
        assert_eq!(telex_keys("đăng"), "ddawng");
        assert_eq!(telex_keys("đường"), "dduwowngf");
        assert_eq!(telex_keys("Nguyễn"), "Nguyeenx");
        assert_eq!(telex_keys("chuyên"), "chuyeen");
        assert_eq!(telex_keys("VIỆT"), "VIEETJ");

        assert_eq!(vni_keys("tiếng"), "tie6ng1");
        assert_eq!(vni_keys("Việt"), "Vie6t5");
        assert_eq!(vni_keys("đăng"), "d9a8ng");
        assert_eq!(vni_keys("đường"), "d9u7o7ng2");
        assert_eq!(vni_keys("Nguyễn"), "Nguye6n4");
    }

    /// A phrase's tone keys land inside their own syllable, not at the end of
    /// the line.
    #[test]
    fn a_space_flushes_the_pending_tone_key() {
        assert_eq!(telex_keys("tiếng Việt"), "tieengs Vieetj");
        assert_eq!(vni_keys("tiếng Việt"), "tie6ng1 Vie6t5");
        assert_eq!(telex_keys("xin chào"), "xin chaof");
    }

    #[test]
    fn a_word_with_nothing_to_spell_is_left_alone() {
        assert_eq!(telex_keys("ban"), "ban");
        assert_eq!(vni_keys("ban"), "ban");
        assert_eq!(telex_keys(""), "");
    }
}
