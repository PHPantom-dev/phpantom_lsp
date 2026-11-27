//! `parse_url()` over a literal URL: the components PHP cuts out of it, each
//! as the literal it is. Mirrors PHPStan's
//! `ParseUrlFunctionDynamicReturnTypeExtension`, which runs the call at
//! analysis time.
//!
//! The parser is a port of PHP's `php_url_parse_ex2()` (`ext/standard/url.c`),
//! so a URL PHP rejects is `false` here too. A URL holding a control
//! character is left to the declared type, since PHP rewrites those to `_`
//! inside each component.

use crate::php_type::{PhpType, ShapeEntry};

use super::{CallArgs, literal_strings};

/// `PHP_URL_*`, as defined by `ext/standard/url.h`, in the order PHP adds the
/// components to the array it returns.
const COMPONENTS: [(&str, i64); 8] = [
    ("PHP_URL_SCHEME", 0),
    ("PHP_URL_HOST", 1),
    ("PHP_URL_PORT", 2),
    ("PHP_URL_USER", 3),
    ("PHP_URL_PASS", 4),
    ("PHP_URL_PATH", 5),
    ("PHP_URL_QUERY", 6),
    ("PHP_URL_FRAGMENT", 7),
];

/// The array keys PHP uses for each component, indexed like [`COMPONENTS`].
const KEYS: [&str; 8] = [
    "scheme", "host", "port", "user", "pass", "path", "query", "fragment",
];

/// One URL's components, indexed like [`COMPONENTS`]. The port's slot is
/// always `None`: the port itself is the second field.
struct Parsed<'a> {
    parts: [Option<&'a [u8]>; 8],
    port: Option<u32>,
}

/// How many literal URLs a call may fold before it is left to the declared
/// type.
const MAX_URLS: usize = 16;

pub(super) fn return_type(args: &CallArgs<'_>) -> Option<PhpType> {
    let urls = literal_strings(&args.resolve(&args.text("$url")?)?)?;
    if urls.is_empty() || urls.len() > MAX_URLS {
        return None;
    }
    let component = match args.text("$component") {
        None => -1,
        Some(text) => component_value(args, &text)?,
    };
    if !(-1..=7).contains(&component) {
        // An unknown component is a `ValueError`, which the declared
        // conditional already says.
        return None;
    }
    let mut results: Vec<PhpType> = Vec::new();
    for url in &urls {
        let ty = match parse(url.as_bytes())? {
            None => PhpType::false_(),
            Some(parsed) => component_type(&parsed, component),
        };
        if !results.contains(&ty) {
            results.push(ty);
        }
    }
    Some(PhpType::union(results))
}

fn component_value(args: &CallArgs<'_>, text: &str) -> Option<i64> {
    let text = text.trim();
    let bare = text.trim_start_matches('\\');
    if let Some(&(_, value)) = COMPONENTS.iter().find(|(name, _)| *name == bare) {
        return Some(value);
    }
    crate::php_type::parse_php_int_literal(text)
        .or_else(|| super::literal_int(&args.resolve(text)?))
}

fn component_type(parsed: &Parsed<'_>, component: i64) -> PhpType {
    let value = |index: usize| -> Option<PhpType> {
        if index == 2 {
            return parsed
                .port
                .map(|port| PhpType::literal_int(port.to_string()));
        }
        parsed.parts[index]
            .map(|bytes| PhpType::literal_string_value(String::from_utf8_lossy(bytes)))
    };
    if component < 0 {
        let entries = (0..KEYS.len())
            .filter_map(|index| {
                Some(ShapeEntry {
                    key: Some(KEYS[index].to_string()),
                    optional: false,
                    value_type: value(index)?,
                })
            })
            .collect();
        return PhpType::array_shape(entries);
    }
    value(component as usize).unwrap_or_else(PhpType::null)
}

/// Parse `url` the way PHP does: `Some(None)` for a URL PHP rejects,
/// `Some(Some(parts))` for one it accepts, and `None` for one this port
/// cannot vouch for.
fn parse(url: &[u8]) -> Option<Option<Parsed<'_>>> {
    if url.iter().any(|&b| b < 0x20 || b == 0x7f) {
        return None;
    }
    Some(parse_bytes(url))
}

/// Where `php_url_parse_ex2()` jumps to after reading the scheme.
enum Next {
    Port(usize),
    Host,
    Path,
}

/// `None` for a URL PHP rejects.
fn parse_bytes(url: &[u8]) -> Option<Parsed<'_>> {
    let ue = url.len();
    let mut ret: [Option<&[u8]>; 8] = Default::default();
    let mut port: Option<u32> = None;
    let mut s = 0usize;
    let starts_with_slashes = |at: usize| at + 1 < ue && url[at] == b'/' && url[at + 1] == b'/';

    let colon = url.iter().position(|&b| b == b':');
    let next = match colon {
        Some(e) if e != 0 => 'scheme: {
            let valid_scheme = url[..e]
                .iter()
                .all(|&b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'.' | b'-'));
            if !valid_scheme {
                let question = url.iter().position(|&b| b == b'?');
                if e + 1 < ue && question.is_some_and(|q| e < q) {
                    break 'scheme Next::Port(e);
                } else if starts_with_slashes(0) {
                    s = 2;
                    break 'scheme Next::Host;
                }
                break 'scheme Next::Path;
            }
            if e + 1 == ue {
                ret[0] = Some(&url[..e]);
                return Some(Parsed { parts: ret, port });
            }
            if url[e + 1] != b'/' {
                // `a.com:80` is a host and a port, not a scheme.
                let mut p = e + 1;
                while p < ue && url[p].is_ascii_digit() {
                    p += 1;
                }
                if (p == ue || url[p] == b'/') && p - e < 7 {
                    break 'scheme Next::Port(e);
                }
                ret[0] = Some(&url[..e]);
                s = e + 1;
                break 'scheme Next::Path;
            }
            ret[0] = Some(&url[..e]);
            if e + 2 < ue && url[e + 2] == b'/' {
                s = e + 3;
                if url[..e].eq_ignore_ascii_case(b"file") && e + 3 < ue && url[e + 3] == b'/' {
                    // `file:///c:/dir/file.txt` keeps its drive letter.
                    if e + 5 < ue && url[e + 5] == b':' {
                        s = e + 4;
                    }
                    break 'scheme Next::Path;
                }
                Next::Host
            } else {
                s = e + 1;
                Next::Path
            }
        }
        Some(e) => Next::Port(e),
        None if starts_with_slashes(0) => {
            s = 2;
            Next::Host
        }
        None => Next::Path,
    };

    let next = match next {
        Next::Port(e) => {
            let p = e + 1;
            let mut pp = p;
            while pp < ue && pp - p < 6 && url[pp].is_ascii_digit() {
                pp += 1;
            }
            if pp - p > 0 && pp - p < 6 && (pp == ue || url[pp] == b'/') {
                port = Some(strtol_port(&url[p..pp])?);
                if starts_with_slashes(s) {
                    s += 2;
                }
                Next::Host
            } else if p == pp && pp == ue {
                return None;
            } else if starts_with_slashes(s) {
                s += 2;
                Next::Host
            } else {
                Next::Path
            }
        }
        other => other,
    };

    if matches!(next, Next::Host) {
        let e = s + url[s..]
            .iter()
            .position(|&b| matches!(b, b'/' | b'?' | b'#'))
            .unwrap_or(ue - s);
        if let Some(at) = url[s..e].iter().rposition(|&b| b == b'@') {
            let at = s + at;
            match url[s..at].iter().position(|&b| b == b':') {
                Some(colon) => {
                    ret[3] = Some(&url[s..s + colon]);
                    ret[4] = Some(&url[s + colon + 1..at]);
                }
                None => ret[3] = Some(&url[s..at]),
            }
            s = at + 1;
        }
        let ipv6 = s < ue && url[s] == b'[' && e > 0 && url[e - 1] == b']';
        let colon = if ipv6 {
            None
        } else {
            url[s..e].iter().rposition(|&b| b == b':').map(|p| s + p)
        };
        let host_end = match colon {
            Some(p) => {
                if port.is_none() {
                    let digits = &url[p + 1..e];
                    if digits.len() > 5 {
                        return None;
                    }
                    if !digits.is_empty() {
                        port = Some(strtol_port(digits)?);
                    }
                }
                p
            }
            None => e,
        };
        if host_end <= s {
            return None;
        }
        ret[1] = Some(&url[s..host_end]);
        if e == ue {
            return Some(Parsed { parts: ret, port });
        }
        s = e;
    }

    // The path, query and fragment.
    let mut e = ue;
    if let Some(hash) = url[s..e].iter().position(|&b| b == b'#') {
        let hash = s + hash;
        ret[7] = Some(&url[hash + 1..e]);
        e = hash;
    }
    if let Some(question) = url[s..e].iter().position(|&b| b == b'?') {
        let question = s + question;
        ret[6] = Some(&url[question + 1..e]);
        e = question;
    }
    if s < e || s == ue {
        ret[5] = Some(&url[s..e]);
    }
    Some(Parsed { parts: ret, port })
}

/// `strtol()` over the port digits after a host: leading digits, then
/// anything; a port needs at least one digit and must fit in 16 bits.
fn strtol_port(digits: &[u8]) -> Option<u32> {
    let end = digits
        .iter()
        .position(|b| !b.is_ascii_digit())
        .unwrap_or(digits.len());
    if end == 0 {
        return None;
    }
    let value: u32 = std::str::from_utf8(&digits[..end]).ok()?.parse().ok()?;
    (value <= 65535).then_some(value)
}

#[cfg(test)]
#[path = "parse_url_tests.rs"]
mod tests;
