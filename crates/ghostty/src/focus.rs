//! Focus event encoding (mode 1004).

use ghostty_sys as sys;

use crate::{Result, ffi};

ffi::c_enum! {
    /// A window focus change reported when terminal mode 1004 is enabled.
    pub enum FocusEvent {
        /// The terminal gained focus.
        Gained = sys::GHOSTTY_FOCUS_GAINED,
        /// The terminal lost focus.
        Lost = sys::GHOSTTY_FOCUS_LOST,
    }
}

impl FocusEvent {
    /// Encodes this focus change; the caller must check mode 1004 before sending it.
    pub fn encode(self) -> Result<Vec<u8>> {
        encode(self)
    }
}

/// Encodes a focus change; send it only when terminal mode 1004 is enabled.
pub fn encode(event: FocusEvent) -> Result<Vec<u8>> {
    ffi::encode_vec(|buf, cap, written| unsafe {
        sys::ghostty_focus_encode(event.to_raw(), buf, cap, written)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_gained_and_lost() -> Result<()> {
        assert_eq!(FocusEvent::Gained.encode()?, b"\x1b[I");
        assert_eq!(encode(FocusEvent::Lost)?, b"\x1b[O");
        Ok(())
    }
}
