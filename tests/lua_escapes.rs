use std::borrow::Cow;
use unescape_zero_copy::{unescape, Error};

fn lua_escape_sequence(s: &str) -> Result<(Option<char>, &str), Error> {
    let mut chars = s.chars();
    let next = chars.next().ok_or(Error::IncompleteSequence)?;
    match next {
        'a' => Ok((Some('\x07'), chars.as_str())),
        'b' => Ok((Some('\x08'), chars.as_str())),
        'f' => Ok((Some('\x0C'), chars.as_str())),
        'r' => Ok((Some('\r'), chars.as_str())),
        'n' => Ok((Some('\n'), chars.as_str())),
        't' => Ok((Some('\t'), chars.as_str())),
        'v' => Ok((Some('\x0B'), chars.as_str())),
        '"' => Ok((Some('"'), chars.as_str())),
        '\\' => Ok((Some('\\'), chars.as_str())),
        '\'' => Ok((Some('\''), chars.as_str())),
        '\n' => Ok((Some('\n'), chars.as_str())),
        'z' => Ok((None, chars.as_str().trim_start())),
        'x' => {
            if s.len() >= 2 {
                let num = u32::from_str_radix(&s[0..2], 16)?;
                let ch = char::from_u32(num).ok_or(Error::InvalidUnicode(num))?;
                Ok((Some(ch), &s[2..]))
            } else {
                Err(Error::IncompleteUnicode)
            }
        }
        'u' => {
            if chars.next() == Some('{') {
                let (hex, rest) = chars
                    .as_str()
                    .split_once('}')
                    .ok_or(Error::IncompleteUnicode)?;
                let num = u32::from_str_radix(hex, 16)?;
                let ch = char::from_u32(num).ok_or(Error::InvalidUnicode(num))?;
                Ok((Some(ch), rest))
            } else {
                Err(Error::UnknownSequence('u'))
            }
        }
        _ => {
            let count = s.chars().take_while(|n| n.is_digit(10)).count().min(3);
            if count > 0 {
                let num: u32 = s[0..count].parse()?;
                let ch = char::from_u32(num).ok_or(Error::InvalidUnicode(num))?;
                Ok((Some(ch), &s[count..]))
            } else {
                Err(Error::UnknownSequence(next))
            }
        }
    }
}

#[test]
fn lua_escapes() {
    assert_eq!(unescape(lua_escape_sequence, "hello").unwrap(), "hello");
    assert!(matches!(
        unescape(lua_escape_sequence, "hello").unwrap(),
        Cow::Borrowed(_)
    ));
    let s1 = unescape(lua_escape_sequence, r"alo\n123").unwrap();
    let s2 = unescape(lua_escape_sequence, r"\97lo\10\04923").unwrap();
    let s3 = unescape(lua_escape_sequence, "alo\\\n123").unwrap();
    assert_eq!(s1, s2);
    assert_eq!(s2, s3);
    assert_eq!(unescape(lua_escape_sequence, r"\z   a").unwrap(), "a");
    assert_eq!(unescape(lua_escape_sequence, r"\za").unwrap(), "a");
    assert_eq!(unescape(lua_escape_sequence, r"a\\b").unwrap(), "a\\b");
    // \u{...} escape
    assert_eq!(unescape(lua_escape_sequence, r"\u{61}").unwrap(), "a");
    assert!(unescape(lua_escape_sequence, "\\u{café}").is_err());
    assert_eq!(
        unescape(lua_escape_sequence, r"\u{41"),
        Err(Error::IncompleteUnicode)
    );
}
