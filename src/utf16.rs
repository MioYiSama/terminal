//! Conversions between UTF-8 byte offsets and the UTF-16 offsets platform
//! text input (IME) uses. Offsets inside a character round up to its end.

pub fn len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// Byte offset of UTF-16 offset `utf16` in `text`.
pub fn to_byte(text: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (offset, c) in text.char_indices() {
        if units >= utf16 {
            return offset;
        }
        units += c.len_utf16();
    }
    text.len()
}

/// UTF-16 offset of byte offset `byte` in `text`.
pub fn from_byte(text: &str, byte: usize) -> usize {
    let mut units = 0;
    for (offset, c) in text.char_indices() {
        if offset >= byte {
            return units;
        }
        units += c.len_utf16();
    }
    units
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surrogate_pairs_round_trip_and_round_up() {
        // "a" (1 unit), "😀" (2 units, 4 bytes), "é" (1 unit, 2 bytes).
        let text = "a😀é";
        assert_eq!(len(text), 4);
        for (utf16, byte) in [(0, 0), (1, 1), (3, 5), (4, 7)] {
            assert_eq!(to_byte(text, utf16), byte);
            assert_eq!(from_byte(text, byte), utf16);
        }
        // Inside the surrogate pair / multi-byte character: the next boundary.
        assert_eq!(to_byte(text, 2), 5);
        assert_eq!(from_byte(text, 2), 3);
        assert_eq!(to_byte(text, 99), text.len());
    }
}
