#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]
#![forbid(unsafe_code)]

//! Small library to unescape strings, without needing to allocate for strings
//! without escape sequences. Tries to support a variety of languages, including
//! through custom parsers if needed, though it mainly supports C-style escape
//! escape sequences by default.

use core::fmt;
use core::num::ParseIntError;

#[cfg(all(feature = "alloc", not(feature = "std")))]
use alloc::borrow::Cow;
#[cfg(feature = "std")]
use std::borrow::Cow;

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
    #[inline]
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

/// A fragment of an unescaped string.
///
/// This is either the largest string slice between escape sequences or the
/// result of parsing an escape sequence.
pub enum StringFragment<'a> {
    /// A string slice between escape sequences.
    Raw(&'a str),
    /// An unescaped character from an escape sequence.
    Escaped(char),
    /// An escape sequence that produced no character.
    ///
    /// None of the default escape sequences do this, but some languages (e.g.
    /// Lua) have an escape sequence that trims out whitespace or otherwise
    /// doesn't produce a visible character.
    Empty,
}

impl From<char> for StringFragment<'_> {
    #[inline]
    fn from(this: char) -> Self {
        Self::Escaped(this)
    }
}
impl From<Option<char>> for StringFragment<'_> {
    #[inline]
    fn from(this: Option<char>) -> Self {
        match this {
            Some(ch) => Self::Escaped(ch),
            None => Self::Empty,
        }
    }
}
impl<'a> From<&'a str> for StringFragment<'a> {
    #[inline]
    fn from(this: &'a str) -> Self {
        Self::Raw(this)
    }
}

#[inline]
fn unicode_char(s: &str, chars: usize) -> Result<(char, &str), Error> {
    if s.len() < chars {
        Err(Error::IncompleteUnicode)
    } else {
        let num = u32::from_str_radix(&s[0..chars], 16)?;
        let ch = char::from_u32(num).ok_or(Error::InvalidUnicode(num))?;
        Ok((ch, &s[chars..]))
    }
}

/// The default unescaper, focusing on C-style escape sequences.
///
/// Called after a backslash is found. Returns a tuple of the unescaped
/// character and remaining (unconsumed) input.
///
/// Escape sequences supported:
/// * `\a` to a bell character.
/// * `\b` to a backspace.
/// * `\f` to a form feed.
/// * `\n` to a line feed.
/// * `\t` to a (horizontal) tab.
/// * `\v` to a vertical tab.
/// * `\\` to a backslash.
/// * `\'` to a single quote.
/// * `\"` to a double quote.
/// * `\/` to a slash (unescaped per ECMAScript).
/// * `\` followed by a new line keeps the same new line.
/// * `\xNN` to the Unicode character in the two hex digits.
/// * `\uNNNN` as above, but with four hex digits.
/// * `\UNNNNNNNN` as above, but with eight hex digits.
/// * `\u{NN...}` as above, but with variable hex digits.
/// * octal sequences are decoded to the Unicode character.
pub fn default_escape_sequence(s: &str) -> Result<(char, &str), Error> {
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

#[inline]
fn non_empty(s: &str) -> Option<&str> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[inline]
fn split_at_escape(s: &str) -> (Option<&str>, Option<&str>) {
    if let Some((first, last)) = s.split_once('\\') {
        (
            non_empty(first),
            // make sure this one is non-`None` to correctly error on incomplete
            // escape sequences (i.e. strings ending in an unescaped backslash)
            Some(last),
        )
    } else {
        (non_empty(s), None)
    }
}

/// An iterator producing unescaped characters of a string.
///
/// The escape sequences are parsed according to the function provided at the
/// unescaper's creation.
#[derive(Clone, Debug)]
// default type for backwards compatibility
pub struct Unescape<'a, F, E, C = Option<char>>
where
    F: FnMut(&'a str) -> Result<(C, &'a str), E>,
    C: From<char>,
{
    bare: Option<&'a str>,
    escaped: Option<&'a str>,
    rem: Option<&'a str>,
    escape_sequence: F,
}

impl<'a, F, E, C> Unescape<'a, F, E, C>
where
    F: FnMut(&'a str) -> Result<(C, &'a str), E>,
    C: From<char>,
{
    /// Make a new unescaper over the given string using the given escape
    /// sequence parser.
    ///
    /// The escape sequence parser is called _after_ a backslash has been found,
    /// and shouldn't check for the presence of one. The tuple in the `Result`
    /// should contain the character returned and the remaining portion of the
    /// string after parsing; e.g. a string `"\nabc"` should return
    /// `('\n', "abc")`.
    #[inline]
    pub fn new(escape_sequence: F, from: &'a str) -> Self {
        let rem = non_empty(from);
        let (bare, escaped) = split_at_escape(from);
        Self {
            bare,
            escaped,
            rem,
            escape_sequence,
        }
    }

    /// Returns the unparsed remainder of the string.
    ///
    /// If all the string has been consumed, returns `None`.
    #[inline]
    pub fn remainder(&self) -> Option<&'a str> {
        self.rem
    }
}

impl<'a, F, E, C> Unescape<'a, F, E, C>
where
    F: FnMut(&'a str) -> Result<(C, &'a str), E>,
    C: From<char>,
    StringFragment<'a>: From<C>,
{
    /// Get the next string fragment rather than just the next character.
    ///
    /// Advances the iterator accordingly.
    #[inline]
    pub fn next_fragment(&mut self) -> Option<Result<StringFragment<'a>, E>> {
        if let Some(frag) = self.bare.take() {
            self.rem = self.rem.and_then(|rem| non_empty(&rem[frag.len()..]));
            Some(Ok(StringFragment::Raw(frag)))
        } else {
            self.next().map(|opt| opt.map(StringFragment::from))
        }
    }

    /// Processes the rest of the iterator into a [`Cow`] string.
    ///
    /// Avoids allocation unless any escape sequences were found and had to be
    /// processed; raw strings are returned as-is.
    #[cfg(any(feature = "std", feature = "alloc"))]
    pub fn as_cow(&mut self) -> Result<Cow<'a, str>, E> {
        let mut out = Cow::default();
        while let Some(fragment) = self.next_fragment().transpose()? {
            match fragment {
                StringFragment::Raw(s) => out += s,
                StringFragment::Escaped(c) => out.to_mut().push(c),
                StringFragment::Empty => (),
            }
        }
        Ok(out)
    }
}

impl<'a, F, E, C> Iterator for Unescape<'a, F, E, C>
where
    F: FnMut(&'a str) -> Result<(C, &'a str), E>,
    C: From<char>,
{
    type Item = Result<C, E>;
    fn next(&mut self) -> Option<Self::Item> {
        self.bare
            .as_mut()
            .and_then(|bare| {
                let mut chars = bare.chars();
                let ch = chars.next()?;
                *bare = chars.as_str();
                self.rem = self.rem.and_then(|rem| non_empty(&rem[ch.len_utf8()..]));
                Some(Ok(C::from(ch)))
            })
            .or_else(|| {
                if let Some(s) = self.escaped.take() {
                    Some(match (self.escape_sequence)(s) {
                        Ok((ch, rem)) => {
                            self.rem = non_empty(rem);
                            let (bare, escaped) = split_at_escape(rem);
                            self.bare = bare;
                            self.escaped = escaped;
                            Ok(ch)
                        }
                        Err(e) => {
                            // assume the error will be reproducible (the escape
                            // sequence parsers should be deterministic), and any
                            // state advancement from here would be invalid anyway,
                            // so abort the unescaper
                            self.bare = None;
                            self.escaped = None;
                            Err(e)
                        }
                    })
                } else {
                    // in case it was empty but not `None`
                    self.bare = None;
                    None
                }
            })
    }
}
impl<'a, F, E, C> core::iter::FusedIterator for Unescape<'a, F, E, C>
where
    F: FnMut(&'a str) -> Result<(C, &'a str), E>,
    C: From<char>,
{
}

// type alias for backwards compatibility
/// An iterator producing unescaped characters of a string.
pub type UnescapeDefault<'a> =
    Unescape<'a, fn(&'a str) -> Result<(char, &'a str), Error>, Error, char>;

/// Unescape the string into a [`Cow`] string.
///
/// The function only allocates if any escape sequences were found; otherwise,
/// the original string is returned unchanged.
/// More information on the escape sequence parser which is provided here can
/// be found at [`Unescape::new`].
#[cfg(any(feature = "std", feature = "alloc"))]
#[inline]
pub fn unescape<'a, F, E, C>(escape_sequence: F, s: &'a str) -> Result<Cow<'a, str>, E>
where
    F: FnMut(&'a str) -> Result<(C, &'a str), E>,
    C: From<char>,
    StringFragment<'a>: From<C>,
{
    Unescape::new(escape_sequence, s).as_cow()
}

/// Unescapes the string as [`unescape`] using the default escape sequence
/// parser.
///
/// See [`default_escape_sequence`] for a list of the supported escape
/// sequences.
#[cfg(any(feature = "std", feature = "alloc"))]
#[inline]
pub fn unescape_default(s: &str) -> Result<Cow<str>, Error> {
    UnescapeDefault::new(default_escape_sequence, s).as_cow()
}

#[cfg(all(test, feature = "std"))]
mod test {
    use super::*;
    use quickcheck::TestResult;
    use quickcheck_macros::quickcheck;
    use std::borrow::Cow;

    #[test]
    fn borrow_strings_without_escapes() {
        assert!(matches!(
            unescape_default("hello").unwrap(),
            Cow::Borrowed(_)
        ));
        assert!(matches!(
            unescape_default("longer\nstring").unwrap(),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn unescapes_backslashes() {
        assert_eq!(unescape_default(r"\\").unwrap(), "\\");
        assert_eq!(unescape_default(r"\\\\").unwrap(), "\\\\");
        assert_eq!(unescape_default(r"\\\\\\").unwrap(), "\\\\\\");
        assert_eq!(unescape_default(r"\\a").unwrap(), "\\a");
        assert!(matches!(
            unescape_default(r"\\\"),
            Err(Error::IncompleteSequence)
        ));
    }

    #[test]
    fn unicode_escapes() {
        assert_eq!(unescape_default(r"\u1234").unwrap(), "\u{1234}");
        assert_eq!(unescape_default(r"\u{1234}").unwrap(), "\u{1234}");
        assert_eq!(unescape_default(r"\U0010FFFF").unwrap(), "\u{10FFFF}");
        assert_eq!(unescape_default(r"\x20").unwrap(), " ");
    }

    #[quickcheck]
    #[cfg_attr(miri, ignore = "slow to run under Miri")]
    fn inverts_escape_default(s: String) -> TestResult {
        let escaped: String = s.escape_default().collect();
        if escaped == s {
            // only bother testing strings that need escaped
            return TestResult::discard();
        }
        let unescaped = unescape_default(&escaped);
        match unescaped {
            Ok(unescaped) => TestResult::from_bool(s == unescaped),
            Err(e) => TestResult::error(e.to_string()),
        }
    }
}
#[cfg(all(test, not(feature = "std")))]
compile_error!("Tests currently require `std` feature");
