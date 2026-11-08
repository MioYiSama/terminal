//! Unicode widths matching Ghostty's terminal layout rules.

use ghostty_sys as sys;

/// Returns a codepoint's width in terminal cells (0, 1, or 2).
///
/// Controls, combining marks, default-ignorable characters, and surrogates
/// have width zero. Invalid codepoints above U+10FFFF have width one. For
/// cluster-level variation selectors and emoji, use [`grapheme_width`].
pub fn codepoint_width(codepoint: u32) -> u8 {
    unsafe { sys::ghostty_unicode_codepoint_width(codepoint) }
}

/// Returns `(codepoints_consumed, cell_width)` for the first grapheme cluster.
///
/// Models grapheme clustering enabled (terminal mode 2027). Empty input returns
/// `(0, 0)`; nonempty input consumes at least one codepoint. The input must
/// contain a complete first cluster or the logical end of the string. If this
/// consumes all of a streaming chunk, buffer more input before treating that
/// cluster as final: a subsequent codepoint can extend it and change its width.
/// This function allocates nothing and is thread-safe.
pub fn grapheme_width(codepoints: &[u32]) -> (usize, u8) {
    let mut width = 0;
    let consumed = unsafe {
        sys::ghostty_unicode_grapheme_width(codepoints.as_ptr(), codepoints.len(), &mut width)
    };
    (consumed, width)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codepoint_widths() {
        assert_eq!(codepoint_width('a' as u32), 1);
        assert_eq!(codepoint_width('中' as u32), 2);
        assert_eq!(codepoint_width('\u{0301}' as u32), 0);
        assert_eq!(codepoint_width(0x200d), 0);
        assert_eq!(codepoint_width(0xd800), 0);
        assert_eq!(codepoint_width(0x110000), 1);
    }

    #[test]
    fn grapheme_clusters_and_emoji_zwj() {
        assert_eq!(grapheme_width(&[]), (0, 0));
        assert_eq!(grapheme_width(&['a' as u32, 0x0301, 'b' as u32]), (2, 1));
        // Woman technologist followed by another independent grapheme.
        assert_eq!(
            grapheme_width(&[0x1f469, 0x200d, 0x1f4bb, 'a' as u32]),
            (3, 2)
        );
        assert_eq!(grapheme_width(&[0x2764, 0xfe0f]), (2, 2));
        assert_eq!(grapheme_width(&[0x110000, 'a' as u32]), (1, 1));
    }
}
