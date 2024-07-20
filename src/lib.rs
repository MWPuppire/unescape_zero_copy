#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]

//! Small library to unescape strings. Tries to support a variety of languages,
//! though it mainly supports C-style escape sequences.

//! Escape sequences supported:
//! * `\a` to a bell character.
//! * `\b` to a backspace.
//! * `\f` to a form feed.
//! * `\n` to a line feed.
//! * `\t` to a (horizontal) tab.
//! * `\v` to a vertical tab.
//! * `\\` to a backslash.
//! * `\'` to a single quote.
//! * `\"` to a double quote.
//! * `\/` to a slash (unescaped per ECMAScript).
//! * `\` followed by a new line keeps the same new line.
//! * `\xNN` to the Unicode character in the two hex digits.
//! * `\uNNNN` as above, but with four hex digits.
//! * `\UNNNNNNNN` as above, but with eight hex digits.
//! * `\u{NN...}` as above, but with variable hex digits.
//! * octal sequences are decoded to the Unicode character.

use core::fmt;
use core::num::ParseIntError;

/// Errors which may be returned by the unescaper.
#[derive(Debug, PartialEq)]
pub enum Error {
    /// Error type for a string ending in a backslash without a following escape
    /// sequence.
    IncompleteSequence,
    /// Error type for a string ending in a Unicode escape sequence (e.g. `\x`)
    /// without the appropriate amount of hex digits.
    IncompleteUnicode,
    /// Error type for a Unicode sequence without a valid character code.
    InvalidUnicode(u32),
    /// Error type for unknown escape sequences.
    UnknownSequence(char),
    /// Errors from parsing Unicode hexadecimal numbers.
    ParseIntError(ParseIntError),
}

impl From<ParseIntError> for Error {
    fn from(this: ParseIntError) -> Self {
        Error::ParseIntError(this)
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::IncompleteSequence => f.write_str("unexpected end of string after `\\`"),
            Self::IncompleteUnicode => {
                f.write_str("unexpected end of string in Unicode escape sequence")
            }
            Self::InvalidUnicode(code) => write!(f, "invalid Unicode character code {code}"),
            Self::UnknownSequence(ch) => write!(f, "unknown escape sequence starting with `{ch}`"),
            Self::ParseIntError(err) => write!(f, "error parsing integer: {err}"),
        }
    }
}
#[cfg(feature = "std")]
impl std::error::Error for Error {}

/// A fragment of a string, either an escaped character or the largest string
/// slice before the next escape sequence.
pub enum StringFragment<'a> {
    /// A string slice between escape sequences.
    Raw(&'a str),
    /// An unescaped character from an escape sequence.
    Escaped(char),
}

fn unicode_char(s: &str, chars: usize) -> Result<(char, &str), Error> {
    if s.len() < chars {
        Err(Error::IncompleteUnicode)
    } else {
        let num = u32::from_str_radix(&s[0..chars], 16)?;
        let ch = char::from_u32(num).ok_or(Error::InvalidUnicode(num))?;
        Ok((ch, &s[chars..]))
    }
}

// called after encountering the backslash
fn escape_sequence(s: &str) -> Result<(char, &str), Error> {
    let mut chars = s.chars();
    let next = chars.next().ok_or(Error::IncompleteSequence)?;
    match next {
        'a' => Ok(('\x07', chars.as_str())),
        'b' => Ok(('\x08', chars.as_str())),
        'f' => Ok(('\x0C', chars.as_str())),
        'n' => Ok(('\n', chars.as_str())),
        'r' => Ok(('\r', chars.as_str())),
        't' => Ok(('\t', chars.as_str())),
        'v' => Ok(('\x0B', chars.as_str())),
        '\\' | '\'' | '\"' | '/' => Ok((next, chars.as_str())),
        '\r' | '\n' => Ok((next, chars.as_str())),
        'x' => unicode_char(chars.as_str(), 2),
        'u' => {
            let s = chars.as_str();
            if chars.next() == Some('{') {
                let s = chars.as_str();
                let size = chars.by_ref().take_while(|n| *n != '}').count();
                let num = u32::from_str_radix(&s[0..size], 16)?;
                let ch = char::from_u32(num).ok_or(Error::InvalidUnicode(num))?;
                Ok((ch, chars.as_str()))
            } else {
                unicode_char(s, 4)
            }
        }
        'U' => unicode_char(chars.as_str(), 8),
        _ => {
            let count = s.chars().take_while(|n| n.is_digit(8)).count().min(3);
            if count > 0 {
                let num = u32::from_str_radix(&s[0..count], 8)?;
                let ch = char::from_u32(num).ok_or(Error::InvalidUnicode(num))?;
                Ok((ch, &s[count..]))
            } else {
                Err(Error::UnknownSequence(next))
            }
        }
    }
}

/// An iterator producing unescaped characters of a string.
pub struct Unescaped<'a> {
    split: core::str::Split<'a, char>,
    rem: Option<core::str::Chars<'a>>,
}

impl<'a> Unescaped<'a> {
    /// Make a new unescaper over the given string.
    pub fn new(from: &'a str) -> Self {
        let mut split = from.split('\\');
        let rem = split
            .next()
            .and_then(|s| if s.is_empty() { None } else { Some(s.chars()) });
        Self { split, rem }
    }

    /// Get the next string fragment rather than just the next character.
    /// Advances the iterator accordingly.
    pub fn next_fragment(&mut self) -> Option<Result<StringFragment<'a>, Error>> {
        if let Some(rem) = self.rem.take() {
            let s = rem.as_str();
            Some(Ok(StringFragment::Raw(s)))
        } else {
            self.next().map(|opt| opt.map(StringFragment::Escaped))
        }
    }

    fn next_escape_sequence(&mut self, next: &'a str) -> Result<char, Error> {
        match escape_sequence(next) {
            Ok((ch, rem)) => {
                if !rem.is_empty() {
                    self.rem = Some(rem.chars());
                }
                Ok(ch)
            }
            Err(e) => Err(e),
        }
    }
}

impl<'a> Iterator for Unescaped<'a> {
    type Item = Result<char, Error>;
    fn next(&mut self) -> Option<Result<char, Error>> {
        if let Some(ref mut rem) = self.rem {
            if let Some(next) = rem.next() {
                Some(Ok(next))
            } else {
                self.rem = None;
                self.next()
            }
        } else {
            let next = self.split.next()?;
            if next.is_empty() {
                match self.split.next() {
                    None => Some(Err(Error::IncompleteSequence)),
                    Some("") => Some(Ok('\\')),
                    Some(s) => {
                        self.rem = Some(s.chars());
                        Some(Ok('\\'))
                    }
                }
            } else {
                Some(self.next_escape_sequence(next))
            }
        }
    }
}
impl<'a> core::iter::FusedIterator for Unescaped<'a> {}

/// Unescape the string into a [`std::borrow::Cow`] string which only allocates
/// if any escape sequences were found; otherwise, the original string is
/// returned unchanged.
#[cfg(feature = "std")]
pub fn unescape(s: &str) -> Result<std::borrow::Cow<str>, Error> {
    let mut out = std::borrow::Cow::default();
    let mut unescaped = Unescaped::new(s);
    while let Some(fragment) = unescaped.next_fragment().transpose()? {
        match fragment {
            StringFragment::Raw(s) => out += s,
            StringFragment::Escaped(c) => out.to_mut().push(c),
        }
    }
    Ok(out)
}

#[cfg(all(test, feature = "std"))]
mod test {
    use quickcheck::TestResult;
    use quickcheck_macros::quickcheck;
    use super::*;
    use std::borrow::Cow;

    #[test]
    fn borrow_strings_without_escapes() {
        assert!(matches!(unescape("hello").unwrap(), Cow::Borrowed(_)));
        assert!(matches!(unescape("longer\nstring").unwrap(), Cow::Borrowed(_)));
    }

    #[test]
    fn unescapes_backslashes() {
        assert_eq!(unescape(r"\\").unwrap(), "\\");
        assert_eq!(unescape(r"\\\\").unwrap(), "\\\\");
        assert_eq!(unescape(r"\\\\\\").unwrap(), "\\\\\\");
        assert_eq!(unescape(r"\\a").unwrap(), "\\a");
        assert_eq!(unescape(r"\\\"), Err(Error::IncompleteSequence));
    }

    #[test]
    fn unicode_escapes() {
        assert_eq!(unescape(r"\u1234").unwrap(), "\u{1234}");
        assert_eq!(unescape(r"\u{1234}").unwrap(), "\u{1234}");
        assert_eq!(unescape(r"\U0010FFFF").unwrap(), "\u{10FFFF}");
        assert_eq!(unescape(r"\x20").unwrap(), " ");
    }

    #[quickcheck]
    fn inverts_escape_default(s: String) -> TestResult {
        let escaped: String = s.escape_default().collect();
        if escaped == s {
            // only bother testing strings that need escaped
            return TestResult::discard();
        }
        let unescaped = unescape(&escaped);
        match unescaped {
            Ok(unescaped) => TestResult::from_bool(s == unescaped),
            Err(e) => TestResult::error(e.to_string()),
        }
    }
}
#[cfg(all(test, not(feature = "std")))]
compile_error!("Tests currently require `std` feature");
