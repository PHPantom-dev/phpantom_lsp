//! How `mb_strlen()` counts a string in each encoding mbstring knows.
//!
//! A literal in PHP source is UTF-8, but `mb_strlen()` reads its bytes in
//! whatever encoding the call names: `'école'` is 5 characters in UTF-8, 6
//! in ISO-8859-1, 3 in UTF-16 and 1 in UTF-32. Each [`Family`] groups the
//! encodings that count the same way, modelled on how mbstring (PHP 8.3 and
//! later) splits bytes into characters, invalid bytes included. Where a
//! family would have to decode an escape syntax (`+` in UTF-7, `&` in
//! HTML-ENTITIES, the shift sequences of ISO-2022) or recover from an
//! invalid multi-byte form whose recovery is not modelled, the count
//! declines rather than guess.

/// A set of encodings `mb_strlen()` counts the same way.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Family {
    /// One character per byte: the single-byte code pages, `8bit`, and the
    /// 7-bit and transfer encodings that read a byte they cannot decode as
    /// one invalid character.
    Bytes,
    Utf8,
    /// The mobile carriers' UTF-8 variants, which decode some of their
    /// private-use emoji to a pair of code points.
    Utf8Docomo,
    Utf8KddiA,
    Utf8KddiB,
    Utf8Softbank,
    /// UTF-16 without a byte-order mark reads big-endian.
    Utf16Be,
    Utf16Le,
    /// UCS-2 in either byte order: two bytes a character, a trailing odd
    /// byte dropped.
    Ucs2,
    /// UTF-32 and UCS-4 in either byte order: four bytes a character, the
    /// remainder dropped.
    Ucs4,
    Sjis,
    /// CP932 and the Shift_JIS variants that share its lead bytes.
    SjisWin,
    SjisSoftbank,
    SjisKddi,
    Sjis2004,
    SjisMac,
    /// EUC-JP and eucJP-win.
    EucJp,
    EucJp2004,
    Cp51932,
    EucCn,
    EucKr,
    Big5,
    /// CP936 (GBK) and CP950: a byte from `0x81` up starts a pair.
    DoubleByte,
    Uhc,
    Gb18030,
    EucTw,
    Base64,
    QuotedPrintable,
    Utf7,
    Utf7Imap,
    HtmlEntities,
    Hz,
    /// The ISO-2022-JP family (JIS, CP50220-2 and the rest).
    Iso2022Jp,
    Iso2022Kr,
}

/// Every family, for a call whose encoding is not known.
pub(super) const ALL_FAMILIES: &[Family] = &[
    Family::Bytes,
    Family::Utf8,
    Family::Utf8Docomo,
    Family::Utf8KddiA,
    Family::Utf8KddiB,
    Family::Utf8Softbank,
    Family::Utf16Be,
    Family::Utf16Le,
    Family::Ucs2,
    Family::Ucs4,
    Family::Sjis,
    Family::SjisWin,
    Family::SjisSoftbank,
    Family::SjisKddi,
    Family::Sjis2004,
    Family::SjisMac,
    Family::EucJp,
    Family::EucJp2004,
    Family::Cp51932,
    Family::EucCn,
    Family::EucKr,
    Family::Big5,
    Family::DoubleByte,
    Family::Uhc,
    Family::Gb18030,
    Family::EucTw,
    Family::Base64,
    Family::QuotedPrintable,
    Family::Utf7,
    Family::Utf7Imap,
    Family::HtmlEntities,
    Family::Hz,
    Family::Iso2022Jp,
    Family::Iso2022Kr,
];

/// The family an encoding name (or alias) belongs to, matched the way
/// mbstring matches names: case-insensitively.
pub(super) fn family_of(name: &str) -> Option<Family> {
    ENCODINGS
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
        .map(|&(_, family)| family)
}

/// How many characters `mb_strlen()` counts in `text` read as `family`,
/// or `None` when the count would depend on decoding this model does not
/// do.
pub(super) fn length(text: &str, family: Family) -> Option<usize> {
    let bytes = text.as_bytes();
    let declines_on = |special: &[u8]| -> Option<usize> {
        (!bytes.iter().any(|b| special.contains(b))).then_some(bytes.len())
    };
    match family {
        Family::Bytes => Some(bytes.len()),
        Family::Utf8 => Some(text.chars().count()),
        Family::Utf8Docomo => Some(utf8_mobile_length(text, UTF8_DOCOMO_DOUBLES)),
        Family::Utf8KddiA => Some(utf8_mobile_length(text, UTF8_KDDI_A_DOUBLES)),
        Family::Utf8KddiB => Some(utf8_mobile_length(text, UTF8_KDDI_B_DOUBLES)),
        Family::Utf8Softbank => Some(utf8_mobile_length(text, UTF8_SOFTBANK_DOUBLES)),
        Family::Utf16Be => Some(utf16_length(bytes, u16::from_be_bytes)),
        Family::Utf16Le => Some(utf16_length(bytes, u16::from_le_bytes)),
        Family::Ucs2 => Some(bytes.len() / 2),
        Family::Ucs4 => Some(bytes.len() / 4),
        Family::Sjis => pairs(bytes, |lead| matches!(lead, 0x81..=0x9F | 0xE0..=0xEF)),
        Family::SjisWin => pairs(bytes, is_sjis_win_lead),
        Family::SjisSoftbank => {
            // SoftBank's emoji escapes (`ESC $ …`) are decoded.
            declines_on(&[0x1B])?;
            pairs(bytes, is_sjis_win_lead)
        }
        Family::SjisKddi => walk(bytes, |lead, rest| {
            pair_step(
                is_sjis_win_lead(lead),
                lead,
                rest,
                SJIS_KDDI_DOUBLES,
                &[],
                &[],
                &[],
            )
        }),
        Family::Sjis2004 => walk(bytes, |lead, rest| {
            pair_step(
                is_sjis_win_lead(lead),
                lead,
                rest,
                SJIS_2004_DOUBLES,
                &[],
                &[],
                &[],
            )
        }),
        Family::SjisMac => walk(bytes, |lead, rest| {
            let is_lead = matches!(lead, 0x81..=0x9F | 0xE0..=0xED);
            pair_step(
                is_lead,
                lead,
                rest,
                SJIS_MAC_DOUBLES,
                SJIS_MAC_TRIPLES,
                SJIS_MAC_QUADRUPLES,
                SJIS_MAC_QUINTUPLES,
            )
        }),
        Family::EucJp => walk(bytes, |lead, _| Step::Take(euc_jp_width(lead), 1)),
        Family::EucJp2004 => walk(bytes, |lead, rest| match lead {
            0x8E => Step::Take(2, 1),
            // A JIS X 0213 plane-2 row takes the two bytes after it; any
            // other byte is taken with the `0x8F` as one invalid character.
            0x8F => match rest.first() {
                Some(0xA1 | 0xA3..=0xA5 | 0xA8 | 0xAC..=0xAF | 0xEE..=0xFE) => Step::Take(3, 1),
                _ => Step::Take(2, 1),
            },
            _ => pair_step(
                matches!(lead, 0xA1..=0xFE),
                lead,
                rest,
                EUC_JP_2004_DOUBLES,
                &[],
                &[],
                &[],
            ),
        }),
        Family::Cp51932 => pairs(bytes, |lead| matches!(lead, 0x8E | 0xA1..=0xFE)),
        Family::EucCn => pairs(bytes, |lead| matches!(lead, 0xA1..=0xA9 | 0xB0..=0xF7)),
        Family::EucKr => pairs(
            bytes,
            |lead| matches!(lead, 0xA1..=0xAC | 0xB0..=0xC8 | 0xCA..=0xFD),
        ),
        // A lead whose pairs are mostly unassigned stands alone before a
        // byte that could start or end another pair.
        Family::Big5 => walk(bytes, |lead, rest| match (lead, rest.first()) {
            (0xC8, Some(0x40..=0x7E | 0xA1..=0xFE)) => Step::Take(1, 1),
            (0xA1..=0xF9, _) => Step::Take(2, 1),
            _ => Step::Take(1, 1),
        }),
        Family::DoubleByte => pairs(bytes, |lead| matches!(lead, 0x81..=0xFE)),
        Family::Uhc => walk(bytes, |lead, rest| match (lead, rest.first()) {
            (0xC9, Some(0x41..=0xFE)) => Step::Take(1, 1),
            (0x81..=0xFD, _) => Step::Take(2, 1),
            _ => Step::Take(1, 1),
        }),
        // The four-byte form (a digit after the lead) is not modelled.
        Family::Gb18030 => walk(bytes, |lead, rest| match (lead, rest.first()) {
            (0x81..=0xFE, Some(b'0'..=b'9')) => Step::Decline,
            (0x81..=0xFE, _) => Step::Take(2, 1),
            _ => Step::Take(1, 1),
        }),
        // Neither is the four-byte form that `0x8E` starts.
        Family::EucTw => walk(bytes, |lead, _| match lead {
            0x8E => Step::Decline,
            0xA1..=0xA6 | 0xC2 | 0xC4..=0xFD => Step::Take(2, 1),
            _ => Step::Take(1, 1),
        }),
        Family::Base64 => Some(base64_length(bytes)),
        Family::QuotedPrintable => declines_on(b"="),
        Family::Utf7 => declines_on(b"+"),
        Family::Utf7Imap | Family::HtmlEntities => declines_on(b"&"),
        Family::Hz => declines_on(b"~"),
        Family::Iso2022Jp | Family::Iso2022Kr => declines_on(&[0x0E, 0x0F, 0x1B]),
    }
}

/// What the character starting at a byte is: how many bytes it spans
/// and how many characters it counts as.
enum Step {
    Take(usize, usize),
    Decline,
}

/// Counts the characters `step` splits `text` into. A character cut short
/// by the end of the string counts once, as mbstring counts it.
fn walk(text: &[u8], step: impl Fn(u8, &[u8]) -> Step) -> Option<usize> {
    let mut count = 0;
    let mut i = 0;
    while i < text.len() {
        match step(text[i], &text[i + 1..]) {
            Step::Take(width, chars) => {
                i += width;
                count += chars;
            }
            Step::Decline => return None,
        }
    }
    Some(count)
}

/// A double-byte encoding where a lead byte always takes the byte after it.
fn pairs(text: &[u8], is_lead: impl Fn(u8) -> bool) -> Option<usize> {
    walk(text, |lead, _| {
        Step::Take(if is_lead(lead) { 2 } else { 1 }, 1)
    })
}

/// A pair, counted as more than one character when it decodes to a
/// sequence (a base letter and a combining mark, or a vendor ligature).
/// Each table lists the pairs, as `lead << 8 | trail`, that count 2, 3, 4
/// and 5.
fn pair_step(
    is_lead: bool,
    lead: u8,
    rest: &[u8],
    doubles: &[u16],
    triples: &[u16],
    quadruples: &[u16],
    quintuples: &[u16],
) -> Step {
    if !is_lead {
        return Step::Take(1, 1);
    }
    let Some(&trail) = rest.first() else {
        return Step::Take(1, 1);
    };
    let code = u16::from(lead) << 8 | u16::from(trail);
    let chars = [(doubles, 2), (triples, 3), (quadruples, 4), (quintuples, 5)]
        .iter()
        .find(|(table, _)| table.contains(&code))
        .map_or(1, |&(_, chars)| chars);
    Step::Take(2, chars)
}

fn utf8_mobile_length(text: &str, doubles: &[u32]) -> usize {
    text.chars()
        .map(|c| {
            if doubles.contains(&u32::from(c)) {
                2
            } else {
                1
            }
        })
        .sum()
}

fn is_sjis_win_lead(lead: u8) -> bool {
    matches!(lead, 0x81..=0x9F | 0xE0..=0xFC)
}

fn euc_jp_width(lead: u8) -> usize {
    match lead {
        0x8F => 3,
        0x8E | 0xA1..=0xFE => 2,
        _ => 1,
    }
}

/// UTF-16 in the byte order `unit` reads: a surrogate pair is one
/// character, and a lone surrogate or a trailing odd byte one invalid
/// character each.
fn utf16_length(text: &[u8], unit: fn([u8; 2]) -> u16) -> usize {
    let mut count = 0;
    let mut i = 0;
    while i + 1 < text.len() {
        let first = unit([text[i], text[i + 1]]);
        i += 2;
        if (0xD800..0xDC00).contains(&first)
            && i + 1 < text.len()
            && (0xDC00..0xE000).contains(&unit([text[i], text[i + 1]]))
        {
            i += 2;
        }
        count += 1;
    }
    count + (text.len() - i)
}

/// BASE64 counts the bytes the text decodes to, skipping whitespace and
/// padding, plus one invalid character for each other byte outside the
/// alphabet.
fn base64_length(text: &[u8]) -> usize {
    let mut sextets = 0;
    let mut invalid = 0;
    for &b in text {
        if b.is_ascii_alphanumeric() || b == b'+' || b == b'/' {
            sextets += 1;
        } else if !matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b'=') {
            invalid += 1;
        }
    }
    sextets * 6 / 8 + invalid
}

/// The carriers' emoji that decode to a pair of code points (a flag or a
/// keycap) in their UTF-8 variants.
const UTF8_DOCOMO_DOUBLES: &[u32] = &[
    0xE6E0, 0xE6E2, 0xE6E3, 0xE6E4, 0xE6E5, 0xE6E6, 0xE6E7, 0xE6E8, 0xE6E9, 0xE6EA, 0xE6EB,
];
const UTF8_KDDI_A_DOUBLES: &[u32] = &[
    0xE4CC, 0xE522, 0xE523, 0xE524, 0xE525, 0xE526, 0xE527, 0xE528, 0xE529, 0xE52A, 0xE573, 0xE5AC,
    0xE5D5, 0xE5D6, 0xEAFA, 0xEB0E, 0xEB0F, 0xEB10, 0xEB11, 0xEB12, 0xEB84,
];
const UTF8_KDDI_B_DOUBLES: &[u32] = &[
    0xEC48, 0xEC49, 0xECCE, 0xECCF, 0xECD0, 0xECD1, 0xECD2, 0xECD3, 0xED89, 0xEFA5, 0xEFFB, 0xEFFC,
    0xF040, 0xF041, 0xF042, 0xF043, 0xF044, 0xF045, 0xF046, 0xF090, 0xF0C9,
];
const UTF8_SOFTBANK_DOUBLES: &[u32] = &[
    0xE210, 0xE21C, 0xE21D, 0xE21E, 0xE21F, 0xE220, 0xE221, 0xE222, 0xE223, 0xE224, 0xE225, 0xE50B,
    0xE50C, 0xE50D, 0xE50E, 0xE50F, 0xE510, 0xE511, 0xE512, 0xE513, 0xE514,
];

/// Shift_JIS-2004 pairs that decode to a kana or letter and a combining
/// mark.
const SJIS_2004_DOUBLES: &[u16] = &[
    0x82F5, 0x82F6, 0x82F7, 0x82F8, 0x82F9, 0x8397, 0x8398, 0x8399, 0x839A, 0x839B, 0x839C, 0x839D,
    0x839E, 0x83F6, 0x8663, 0x8667, 0x8668, 0x8669, 0x866A, 0x866B, 0x866C, 0x866D, 0x866E, 0x8685,
    0x8686,
];

/// The same characters in EUC-JP-2004.
const EUC_JP_2004_DOUBLES: &[u16] = &[
    0xA4F7, 0xA4F8, 0xA4F9, 0xA4FA, 0xA4FB, 0xA5F7, 0xA5F8, 0xA5F9, 0xA5FA, 0xA5FB, 0xA5FC, 0xA5FD,
    0xA5FE, 0xA6F8, 0xABC4, 0xABC8, 0xABC9, 0xABCA, 0xABCB, 0xABCC, 0xABCD, 0xABCE, 0xABCF, 0xABE5,
    0xABE6,
];

/// KDDI emoji that decode to a pair of code points.
const SJIS_KDDI_DOUBLES: &[u16] = &[
    0xF348, 0xF349, 0xF3CE, 0xF3CF, 0xF3D0, 0xF3D1, 0xF3D2, 0xF3D3, 0xF489,
];

/// MacJapanese ligatures and symbols that decode to a sequence.
const SJIS_MAC_DOUBLES: &[u16] = &[
    0x8645, 0x864B, 0x86D3, 0x86D4, 0x86D5, 0x86D6, 0x8791, 0x8792, 0x879D, 0xEB41, 0xEB42, 0xEB50,
    0xEB5B, 0xEB5D, 0xEB60, 0xEB61, 0xEB62, 0xEB63, 0xEB6D, 0xEB6E, 0xEB81, 0xEC9F, 0xECA1, 0xECA3,
    0xECA5, 0xECA7, 0xECC1, 0xECE1, 0xECE3, 0xECE5, 0xECEC, 0xED40, 0xED42, 0xED44, 0xED46, 0xED48,
    0xED62, 0xED83, 0xED85, 0xED87, 0xED8E, 0xED95, 0xED96,
];
const SJIS_MAC_TRIPLES: &[u16] = &[0x8591, 0x85AD, 0x85C1, 0x865D, 0x86CE];
const SJIS_MAC_QUADRUPLES: &[u16] = &[0x85AC, 0x85C0, 0x869E];
const SJIS_MAC_QUINTUPLES: &[u16] = &[0x85AB, 0x85BF, 0x87FB, 0x87FC];

/// Every name `mb_list_encodings()` and `mb_encoding_aliases()` report on
/// PHP 8.5, upper-cased, with the family `mb_strlen()` counts it in.
const ENCODINGS: &[(&str, Family)] = &[
    ("7BIT", Family::Bytes),
    ("8BIT", Family::Bytes),
    ("ANSI_X3.4-1968", Family::Bytes),
    ("ANSI_X3.4-1986", Family::Bytes),
    ("ARABIC", Family::Bytes),
    ("ARMSCII-8", Family::Bytes),
    ("ARMSCII8", Family::Bytes),
    ("ASCII", Family::Bytes),
    ("BASE64", Family::Base64),
    ("BIG-5", Family::Big5),
    ("BIG-FIVE", Family::Big5),
    ("BIGFIVE", Family::Big5),
    ("BINARY", Family::Bytes),
    ("BYTE2BE", Family::Ucs2),
    ("BYTE2LE", Family::Ucs2),
    ("BYTE4BE", Family::Ucs4),
    ("BYTE4LE", Family::Ucs4),
    ("CN-BIG5", Family::Big5),
    ("CN-GB", Family::EucCn),
    ("CP-1251", Family::Bytes),
    ("CP-1254", Family::Bytes),
    ("CP-850", Family::Bytes),
    ("CP-866", Family::Bytes),
    ("CP-936", Family::DoubleByte),
    ("CP1251", Family::Bytes),
    ("CP1252", Family::Bytes),
    ("CP1254", Family::Bytes),
    ("CP367", Family::Bytes),
    ("CP50220", Family::Iso2022Jp),
    ("CP50220-RAW", Family::Iso2022Jp),
    ("CP50220RAW", Family::Iso2022Jp),
    ("CP50221", Family::Iso2022Jp),
    ("CP50222", Family::Iso2022Jp),
    ("CP51932", Family::Cp51932),
    ("CP850", Family::Bytes),
    ("CP866", Family::Bytes),
    ("CP932", Family::SjisWin),
    ("CP936", Family::DoubleByte),
    ("CP949", Family::Uhc),
    ("CP950", Family::DoubleByte),
    ("CSASCII", Family::Bytes),
    ("CYRILLIC", Family::Bytes),
    ("EUC", Family::EucJp),
    ("EUC-CN", Family::EucCn),
    ("EUC-JP", Family::EucJp),
    ("EUC-JP-2004", Family::EucJp2004),
    ("EUC-KR", Family::EucKr),
    ("EUC-TW", Family::EucTw),
    ("EUCCN", Family::EucCn),
    ("EUCJP", Family::EucJp),
    ("EUCJP-MS", Family::EucJp),
    ("EUCJP-OPEN", Family::EucJp),
    ("EUCJP-WIN", Family::EucJp),
    ("EUCKR", Family::EucKr),
    ("EUCTW", Family::EucTw),
    ("EUC_CN", Family::EucCn),
    ("EUC_JP", Family::EucJp),
    ("EUC_JP-2004", Family::EucJp2004),
    ("EUC_KR", Family::EucKr),
    ("EUC_TW", Family::EucTw),
    ("GB-18030", Family::Gb18030),
    ("GB-18030-2000", Family::Gb18030),
    ("GB18030", Family::Gb18030),
    ("GB18030-2022", Family::Gb18030),
    ("GB2312", Family::EucCn),
    ("GBK", Family::DoubleByte),
    ("GREEK", Family::Bytes),
    ("HEBREW", Family::Bytes),
    ("HTML", Family::HtmlEntities),
    ("HTML-ENTITIES", Family::HtmlEntities),
    ("HZ", Family::Hz),
    ("IBM-367", Family::Bytes),
    ("IBM-850", Family::Bytes),
    ("IBM-866", Family::Bytes),
    ("IBM367", Family::Bytes),
    ("IBM850", Family::Bytes),
    ("IBM866", Family::Bytes),
    ("ISO-10646-UCS-2", Family::Ucs2),
    ("ISO-10646-UCS-4", Family::Ucs4),
    ("ISO-2022-JP", Family::Iso2022Jp),
    ("ISO-2022-JP-2004", Family::Iso2022Jp),
    ("ISO-2022-JP-KDDI", Family::Iso2022Jp),
    ("ISO-2022-JP-MOBILE#KDDI", Family::Iso2022Jp),
    ("ISO-2022-JP-MS", Family::Iso2022Jp),
    ("ISO-2022-KR", Family::Iso2022Kr),
    ("ISO-8859-1", Family::Bytes),
    ("ISO-8859-10", Family::Bytes),
    ("ISO-8859-13", Family::Bytes),
    ("ISO-8859-14", Family::Bytes),
    ("ISO-8859-15", Family::Bytes),
    ("ISO-8859-16", Family::Bytes),
    ("ISO-8859-2", Family::Bytes),
    ("ISO-8859-3", Family::Bytes),
    ("ISO-8859-4", Family::Bytes),
    ("ISO-8859-5", Family::Bytes),
    ("ISO-8859-6", Family::Bytes),
    ("ISO-8859-7", Family::Bytes),
    ("ISO-8859-8", Family::Bytes),
    ("ISO-8859-9", Family::Bytes),
    ("ISO-IR-6", Family::Bytes),
    ("ISO2022JPMS", Family::Iso2022Jp),
    ("ISO646-US", Family::Bytes),
    ("ISO8859-1", Family::Bytes),
    ("ISO8859-10", Family::Bytes),
    ("ISO8859-13", Family::Bytes),
    ("ISO8859-14", Family::Bytes),
    ("ISO8859-15", Family::Bytes),
    ("ISO8859-16", Family::Bytes),
    ("ISO8859-2", Family::Bytes),
    ("ISO8859-3", Family::Bytes),
    ("ISO8859-4", Family::Bytes),
    ("ISO8859-5", Family::Bytes),
    ("ISO8859-6", Family::Bytes),
    ("ISO8859-7", Family::Bytes),
    ("ISO8859-8", Family::Bytes),
    ("ISO8859-9", Family::Bytes),
    ("ISO_646.IRV:1991", Family::Bytes),
    ("JIS", Family::Iso2022Jp),
    ("JIS-MS", Family::Iso2022Jp),
    ("KOI8-R", Family::Bytes),
    ("KOI8-U", Family::Bytes),
    ("KOI8R", Family::Bytes),
    ("KOI8U", Family::Bytes),
    ("LATIN1", Family::Bytes),
    ("LATIN2", Family::Bytes),
    ("LATIN3", Family::Bytes),
    ("LATIN4", Family::Bytes),
    ("LATIN5", Family::Bytes),
    ("LATIN6", Family::Bytes),
    ("LATIN8", Family::Bytes),
    ("MACJAPANESE", Family::SjisMac),
    ("MS932", Family::SjisWin),
    ("MS_KANJI", Family::SjisWin),
    ("MUTF-7", Family::Utf7Imap),
    ("QPRINT", Family::QuotedPrintable),
    ("QUOTED-PRINTABLE", Family::QuotedPrintable),
    ("SHIFT-JIS", Family::Sjis),
    ("SHIFT_JIS-2004", Family::Sjis2004),
    ("SHIFT_JIS-IMODE", Family::SjisWin),
    ("SHIFT_JIS-KDDI", Family::SjisKddi),
    ("SHIFT_JIS-SOFTBANK", Family::SjisSoftbank),
    ("SJIS", Family::Sjis),
    ("SJIS-2004", Family::Sjis2004),
    ("SJIS-DOCOMO", Family::SjisWin),
    ("SJIS-KDDI", Family::SjisKddi),
    ("SJIS-MAC", Family::SjisMac),
    ("SJIS-MOBILE#DOCOMO", Family::SjisWin),
    ("SJIS-MOBILE#KDDI", Family::SjisKddi),
    ("SJIS-MOBILE#SOFTBANK", Family::SjisSoftbank),
    ("SJIS-MS", Family::SjisWin),
    ("SJIS-OPEN", Family::SjisWin),
    ("SJIS-SOFTBANK", Family::SjisSoftbank),
    ("SJIS-WIN", Family::SjisWin),
    ("SJIS2004", Family::Sjis2004),
    ("UCS-2", Family::Ucs2),
    ("UCS-2BE", Family::Ucs2),
    ("UCS-2LE", Family::Ucs2),
    ("UCS-4", Family::Ucs4),
    ("UCS-4BE", Family::Ucs4),
    ("UCS-4LE", Family::Ucs4),
    ("UCS2", Family::Ucs2),
    ("UCS4", Family::Ucs4),
    ("UHC", Family::Uhc),
    ("UNICODE", Family::Ucs2),
    ("US", Family::Bytes),
    ("US-ASCII", Family::Bytes),
    ("UTF-16", Family::Utf16Be),
    ("UTF-16BE", Family::Utf16Be),
    ("UTF-16LE", Family::Utf16Le),
    ("UTF-32", Family::Ucs4),
    ("UTF-32BE", Family::Ucs4),
    ("UTF-32LE", Family::Ucs4),
    ("UTF-7", Family::Utf7),
    ("UTF-8", Family::Utf8),
    ("UTF-8-DOCOMO", Family::Utf8Docomo),
    ("UTF-8-KDDI", Family::Utf8KddiB),
    ("UTF-8-MOBILE#DOCOMO", Family::Utf8Docomo),
    ("UTF-8-MOBILE#KDDI", Family::Utf8KddiB),
    ("UTF-8-MOBILE#KDDI-A", Family::Utf8KddiA),
    ("UTF-8-MOBILE#KDDI-B", Family::Utf8KddiB),
    ("UTF-8-MOBILE#SOFTBANK", Family::Utf8Softbank),
    ("UTF-8-SOFTBANK", Family::Utf8Softbank),
    ("UTF16", Family::Utf16Be),
    ("UTF32", Family::Ucs4),
    ("UTF7", Family::Utf7),
    ("UTF7-IMAP", Family::Utf7Imap),
    ("UTF8", Family::Utf8),
    ("UTF8-DOCOMO", Family::Utf8Docomo),
    ("UTF8-KDDI", Family::Utf8KddiB),
    ("UTF8-SOFTBANK", Family::Utf8Softbank),
    ("UUENCODE", Family::Bytes),
    ("WINDOWS-1251", Family::Bytes),
    ("WINDOWS-1252", Family::Bytes),
    ("WINDOWS-1254", Family::Bytes),
    ("WINDOWS-31J", Family::SjisWin),
    ("X-EUC-CN", Family::EucCn),
    ("X-EUC-JP", Family::EucJp),
    ("X-EUC-KR", Family::EucKr),
    ("X-EUC-TW", Family::EucTw),
    ("X-MAC-JAPANESE", Family::SjisMac),
    ("X-SJIS", Family::Sjis),
    ("X-SJIS-EMOJI-DOCOMO", Family::SjisWin),
    ("X-SJIS-EMOJI-KDDI", Family::SjisKddi),
    ("X-SJIS-EMOJI-SOFTBANK", Family::SjisSoftbank),
];
