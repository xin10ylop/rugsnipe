//! Error type shared by every decoder in the crate.

use core::fmt;

/// Why a byte buffer could not be decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// The buffer ended at `offset` while `needed` more bytes were required.
    UnexpectedEof { offset: usize, needed: usize },
    /// The leading 8-byte Anchor discriminator is not the expected one.
    BadDiscriminator { expected: [u8; 8], found: [u8; 8] },
    /// A borsh `bool` byte was neither 0 nor 1.
    InvalidBool { offset: usize, value: u8 },
    /// A borsh string was not valid UTF-8.
    InvalidUtf8 { offset: usize },
    /// A length or count field is out of range for the format.
    InvalidLength { offset: usize, value: usize },
    /// A transaction-format rule was violated (version byte, header, trailing bytes...).
    InvalidTransaction(&'static str),
    /// Base64 in a `Program data:` log line did not decode.
    InvalidBase64,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof { offset, needed } => {
                write!(
                    f,
                    "unexpected end of data at offset {offset}: {needed} more byte(s) needed"
                )
            }
            Self::BadDiscriminator { expected, found } => write!(
                f,
                "bad discriminator: expected {}, found {}",
                hex8(expected),
                hex8(found)
            ),
            Self::InvalidBool { offset, value } => {
                write!(f, "invalid bool byte {value} at offset {offset}")
            }
            Self::InvalidUtf8 { offset } => write!(f, "invalid UTF-8 string at offset {offset}"),
            Self::InvalidLength { offset, value } => {
                write!(f, "invalid length {value} at offset {offset}")
            }
            Self::InvalidTransaction(why) => write!(f, "invalid transaction: {why}"),
            Self::InvalidBase64 => write!(f, "invalid base64"),
        }
    }
}

impl std::error::Error for DecodeError {}

fn hex8(b: &[u8; 8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Result alias used throughout the crate.
pub type Result<T> = core::result::Result<T, DecodeError>;
