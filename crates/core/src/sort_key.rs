//! Order keys compatible with the `fractional-indexing` JS library (base 62). Keys compare as plain
//! strings, and a new key can always be made between two others without touching them.

use serde::{Deserialize, Serialize};

const DIGITS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const SMALLEST_INTEGER: &str = "A00000000000000000000000000";

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SortKey(String);

impl SortKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// A key after `last`, or the first key when there is none.
    pub fn after(last: Option<&SortKey>) -> SortKey {
        let Some(last) = last else { return SortKey("a0".into()) };
        let (int, frac) = last.0.split_at(integer_len(last.0.as_bytes()[0]));
        SortKey(increment(int).unwrap_or_else(|| format!("{int}{}", midpoint_after(frac))))
    }
}

impl TryFrom<String> for SortKey {
    type Error = String;

    fn try_from(s: String) -> Result<Self, String> {
        let b = s.as_bytes();
        let valid = b.first().is_some_and(u8::is_ascii_alphabetic)
            && b.iter().all(u8::is_ascii_alphanumeric)
            && integer_len(b[0]) <= b.len()
            && s != SMALLEST_INTEGER
            && !s[integer_len(b[0])..].ends_with('0');
        if valid { Ok(SortKey(s)) } else { Err(format!("顺序键“{s}”不对")) }
    }
}

impl From<SortKey> for String {
    fn from(k: SortKey) -> String {
        k.0
    }
}

#[cfg(feature = "sql")]
impl rusqlite::ToSql for SortKey {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(self.0.as_str().into())
    }
}

#[cfg(feature = "sql")]
impl rusqlite::types::FromSql for SortKey {
    fn column_result(v: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        Ok(SortKey(v.as_str()?.to_owned()))
    }
}

fn digit(c: u8) -> usize {
    DIGITS.iter().position(|&d| d == c).expect("validated key")
}

/// Length of the integer part, encoded by its first character: `a`–`z` are 2–27, `A`–`Z` are 27–2.
fn integer_len(head: u8) -> usize {
    if head.is_ascii_lowercase() { usize::from(head - b'a') + 2 } else { usize::from(b'Z' - head) + 2 }
}

fn increment(int: &str) -> Option<String> {
    let head = int.as_bytes()[0];
    let mut digits: Vec<u8> = int.bytes().skip(1).collect();
    let mut carry = true;
    for d in digits.iter_mut().rev() {
        let i = digit(*d) + 1;
        if i == DIGITS.len() {
            *d = DIGITS[0];
        } else {
            *d = DIGITS[i];
            carry = false;
            break;
        }
    }
    let head = if carry {
        match head {
            b'Z' => return Some("a0".into()),
            b'z' => return None,
            _ => {}
        }
        let next = head + 1;
        if next > b'a' {
            digits.push(DIGITS[0])
        } else {
            digits.pop();
        }
        next
    } else {
        head
    };
    Some(std::iter::once(head).chain(digits).map(char::from).collect())
}

/// A fraction after `a` (with nothing above it): `a`'s leading top digits, then the digit halfway
/// between the next one and the top.
fn midpoint_after(a: &str) -> String {
    let top = a.bytes().take_while(|&c| digit(c) == DIGITS.len() - 1).count();
    let next = a.as_bytes().get(top).copied().map_or(0, digit);
    format!("{}{}", &a[..top], char::from(DIGITS[(next + DIGITS.len()).div_ceil(2)]))
}

#[cfg(test)]
mod tests {
    use super::SortKey;

    fn after(k: &str) -> String {
        SortKey::after(Some(&SortKey::try_from(k.to_owned()).unwrap())).0
    }

    #[test]
    fn matches_the_js_library() {
        assert_eq!(SortKey::after(None).0, "a0");
        assert_eq!(after("a0"), "a1");
        assert_eq!(after("a9"), "aA");
        assert_eq!(after("az"), "b00");
        assert_eq!(after("Zz"), "a0");
        assert_eq!(after("a0V"), "a1");
        let largest = format!("z{}", "z".repeat(26));
        assert_eq!(after(&largest), format!("{largest}V"));
        // keys only ever get longer here; whatever is generated must read back
        let long = format!("{largest}{}", "z".repeat(100_000));
        assert_eq!(after(&long), format!("{long}V"));
        assert_eq!(after(&format!("{largest}zzA")), format!("{largest}zza"));
    }

    #[test]
    fn keeps_increasing() {
        let mut key = SortKey::after(None);
        for _ in 0..5000 {
            let next = SortKey::after(Some(&key));
            assert!(next > key);
            key = SortKey::try_from(next.0).unwrap();
        }
    }

    #[test]
    fn rejects_malformed_keys() {
        for bad in ["", "a", "b0", "a00", "a0!", "0a", &format!("A{}", "0".repeat(26))] {
            assert!(SortKey::try_from(bad.to_owned()).is_err(), "{bad}");
        }
    }
}
