//! How values the database has no type for are written: a list of texts as one text (each part
//! with its length first, so any text, a separator in it included, comes back as it was), and
//! the model's unsigned numbers as SQLite's signed integers (a number that does not fit is an
//! error, never cut).

use rusqlite::Error;

/// `parts` as one text: `<length>:<part>` each, lengths in bytes.
pub fn pack<S: AsRef<str>>(parts: &[S]) -> String {
    let mut out = String::new();
    for p in parts {
        let p = p.as_ref();
        out.push_str(&p.len().to_string());
        out.push(':');
        out.push_str(p);
    }
    out
}

/// The parts of a text made by [`pack`]; `None` when it is not one.
pub fn unpack(mut text: &str) -> Option<Vec<String>> {
    let mut parts = Vec::new();
    while !text.is_empty() {
        let (len, rest) = text.split_once(':')?;
        let len: usize = len.parse().ok()?;
        let part = rest.get(..len)?;
        parts.push(part.to_string());
        text = &rest[len..];
    }
    Some(parts)
}

/// `n` as SQLite's integer.
pub fn int(n: u64) -> Result<i64, Error> {
    i64::try_from(n).map_err(|e| Error::ToSqlConversionFailure(Box::new(e)))
}

/// An integer of the database as the model's unsigned number; a negative one is damage.
pub fn uint(n: i64) -> Result<u64, Error> {
    u64::try_from(n).map_err(|e| Error::FromSqlConversionFailure(0, rusqlite::types::Type::Integer, Box::new(e)))
}

/// [`uint`], for a count.
pub fn count(n: i64) -> Result<u32, Error> {
    u32::try_from(n).map_err(|e| Error::FromSqlConversionFailure(0, rusqlite::types::Type::Integer, Box::new(e)))
}

#[cfg(test)]
mod tests;
