//! Device attributes (DA1/DA2/DA3) reported by the terminal.
//!
//! Return [`DeviceAttributes`] from the terminal's device attributes callback.
//! The terminal selects the primary, secondary, or tertiary response according
//! to the query. Defaults here are zero-filled, matching the callback's initial
//! output, not the terminal's built-in attributes used when no callback answers.
//! Color scheme response values are in [`crate::color_scheme::ColorScheme`].

use ghostty_sys as sys;

use crate::{Error, Result};

/// DA1 conformance level (`Pp`). Custom numeric values are supported.
///
/// Several terminal models share a conformance level, so these identifiers are
/// constants rather than distinct enum variants.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConformanceLevel(
    /// The conformance level parameter.
    pub u16,
);

impl ConformanceLevel {
    /// VT100 conformance.
    pub const VT100: Self = Self(sys::GHOSTTY_DA_CONFORMANCE_VT100 as u16);
    /// VT101 conformance (the same value as VT100).
    pub const VT101: Self = Self(sys::GHOSTTY_DA_CONFORMANCE_VT101 as u16);
    /// VT102 conformance.
    pub const VT102: Self = Self(sys::GHOSTTY_DA_CONFORMANCE_VT102 as u16);
    /// VT125 conformance.
    pub const VT125: Self = Self(sys::GHOSTTY_DA_CONFORMANCE_VT125 as u16);
    /// VT131 conformance.
    pub const VT131: Self = Self(sys::GHOSTTY_DA_CONFORMANCE_VT131 as u16);
    /// VT132 conformance.
    pub const VT132: Self = Self(sys::GHOSTTY_DA_CONFORMANCE_VT132 as u16);
    /// VT220 conformance (level 2).
    pub const VT220: Self = Self(sys::GHOSTTY_DA_CONFORMANCE_VT220 as u16);
    /// VT240 conformance (level 2).
    pub const VT240: Self = Self(sys::GHOSTTY_DA_CONFORMANCE_VT240 as u16);
    /// VT320 conformance (level 3).
    pub const VT320: Self = Self(sys::GHOSTTY_DA_CONFORMANCE_VT320 as u16);
    /// VT340 conformance (level 3).
    pub const VT340: Self = Self(sys::GHOSTTY_DA_CONFORMANCE_VT340 as u16);
    /// VT420 conformance (level 4).
    pub const VT420: Self = Self(sys::GHOSTTY_DA_CONFORMANCE_VT420 as u16);
    /// VT510 conformance (level 5).
    pub const VT510: Self = Self(sys::GHOSTTY_DA_CONFORMANCE_VT510 as u16);
    /// VT520 conformance (level 5).
    pub const VT520: Self = Self(sys::GHOSTTY_DA_CONFORMANCE_VT520 as u16);
    /// VT525 conformance (level 5).
    pub const VT525: Self = Self(sys::GHOSTTY_DA_CONFORMANCE_VT525 as u16);
    /// Level 2 (VT200 series) conformance.
    pub const LEVEL_2: Self = Self(sys::GHOSTTY_DA_CONFORMANCE_LEVEL_2 as u16);
    /// Level 3 (VT300 series) conformance.
    pub const LEVEL_3: Self = Self(sys::GHOSTTY_DA_CONFORMANCE_LEVEL_3 as u16);
    /// Level 4 (VT400 series) conformance.
    pub const LEVEL_4: Self = Self(sys::GHOSTTY_DA_CONFORMANCE_LEVEL_4 as u16);
    /// Level 5 (VT500 series) conformance.
    pub const LEVEL_5: Self = Self(sys::GHOSTTY_DA_CONFORMANCE_LEVEL_5 as u16);
}

/// A DA1 feature code (`Ps`), including custom numeric codes.
///
/// Feature codes form a list, not a bitmask.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Feature(
    /// The feature's numeric code.
    pub u16,
);

impl Feature {
    /// 132-column operation.
    pub const COLUMNS_132: Self = Self(sys::GHOSTTY_DA_FEATURE_COLUMNS_132 as u16);
    /// Printer support.
    pub const PRINTER: Self = Self(sys::GHOSTTY_DA_FEATURE_PRINTER as u16);
    /// ReGIS graphics.
    pub const REGIS: Self = Self(sys::GHOSTTY_DA_FEATURE_REGIS as u16);
    /// Sixel graphics.
    pub const SIXEL: Self = Self(sys::GHOSTTY_DA_FEATURE_SIXEL as u16);
    /// Selective erase.
    pub const SELECTIVE_ERASE: Self = Self(sys::GHOSTTY_DA_FEATURE_SELECTIVE_ERASE as u16);
    /// User-defined keys.
    pub const USER_DEFINED_KEYS: Self = Self(sys::GHOSTTY_DA_FEATURE_USER_DEFINED_KEYS as u16);
    /// National replacement character sets.
    pub const NATIONAL_REPLACEMENT: Self =
        Self(sys::GHOSTTY_DA_FEATURE_NATIONAL_REPLACEMENT as u16);
    /// Technical character set.
    pub const TECHNICAL_CHARACTERS: Self =
        Self(sys::GHOSTTY_DA_FEATURE_TECHNICAL_CHARACTERS as u16);
    /// Locator support.
    pub const LOCATOR: Self = Self(sys::GHOSTTY_DA_FEATURE_LOCATOR as u16);
    /// Terminal state reporting.
    pub const TERMINAL_STATE: Self = Self(sys::GHOSTTY_DA_FEATURE_TERMINAL_STATE as u16);
    /// Windowing support.
    pub const WINDOWING: Self = Self(sys::GHOSTTY_DA_FEATURE_WINDOWING as u16);
    /// Horizontal scrolling.
    pub const HORIZONTAL_SCROLLING: Self =
        Self(sys::GHOSTTY_DA_FEATURE_HORIZONTAL_SCROLLING as u16);
    /// ANSI color.
    pub const ANSI_COLOR: Self = Self(sys::GHOSTTY_DA_FEATURE_ANSI_COLOR as u16);
    /// Rectangular editing.
    pub const RECTANGULAR_EDITING: Self = Self(sys::GHOSTTY_DA_FEATURE_RECTANGULAR_EDITING as u16);
    /// ANSI text locator.
    pub const ANSI_TEXT_LOCATOR: Self = Self(sys::GHOSTTY_DA_FEATURE_ANSI_TEXT_LOCATOR as u16);
    /// Clipboard support.
    pub const CLIPBOARD: Self = Self(sys::GHOSTTY_DA_FEATURE_CLIPBOARD as u16);
}

/// DA2 terminal type identifier (`Pp`), including custom numeric identifiers.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceType(
    /// The terminal type's numeric identifier.
    pub u16,
);

impl DeviceType {
    /// VT100 terminal type.
    pub const VT100: Self = Self(sys::GHOSTTY_DA_DEVICE_TYPE_VT100 as u16);
    /// VT220 terminal type.
    pub const VT220: Self = Self(sys::GHOSTTY_DA_DEVICE_TYPE_VT220 as u16);
    /// VT240 terminal type.
    pub const VT240: Self = Self(sys::GHOSTTY_DA_DEVICE_TYPE_VT240 as u16);
    /// VT330 terminal type.
    pub const VT330: Self = Self(sys::GHOSTTY_DA_DEVICE_TYPE_VT330 as u16);
    /// VT340 terminal type.
    pub const VT340: Self = Self(sys::GHOSTTY_DA_DEVICE_TYPE_VT340 as u16);
    /// VT320 terminal type.
    pub const VT320: Self = Self(sys::GHOSTTY_DA_DEVICE_TYPE_VT320 as u16);
    /// VT382 terminal type.
    pub const VT382: Self = Self(sys::GHOSTTY_DA_DEVICE_TYPE_VT382 as u16);
    /// VT420 terminal type.
    pub const VT420: Self = Self(sys::GHOSTTY_DA_DEVICE_TYPE_VT420 as u16);
    /// VT510 terminal type.
    pub const VT510: Self = Self(sys::GHOSTTY_DA_DEVICE_TYPE_VT510 as u16);
    /// VT520 terminal type.
    pub const VT520: Self = Self(sys::GHOSTTY_DA_DEVICE_TYPE_VT520 as u16);
    /// VT525 terminal type.
    pub const VT525: Self = Self(sys::GHOSTTY_DA_DEVICE_TYPE_VT525 as u16);
}

/// Primary device attributes, answering `CSI c` (DA1).
///
/// The feature list is bounded to the C response's 64 entries without heap
/// allocation. Its length cannot be changed independently of its contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceAttributesPrimary {
    /// Conformance level (`Pp`), for example [`ConformanceLevel::VT220`].
    pub conformance_level: ConformanceLevel,
    features: [Feature; Self::MAX_FEATURES],
    num_features: usize,
}

impl Default for DeviceAttributesPrimary {
    fn default() -> Self {
        Self {
            conformance_level: ConformanceLevel::default(),
            features: [Feature::default(); Self::MAX_FEATURES],
            num_features: 0,
        }
    }
}

impl DeviceAttributesPrimary {
    /// Maximum number of DA1 feature codes supported by the C response.
    pub const MAX_FEATURES: usize = 64;

    /// Creates a primary response with a conformance level and feature list.
    ///
    /// Returns [`Error::InvalidValue`] if more than 64 features are supplied.
    pub fn new(conformance_level: ConformanceLevel, features: &[Feature]) -> Result<Self> {
        let mut primary = Self {
            conformance_level,
            ..Self::default()
        };
        primary.set_features(features)?;
        Ok(primary)
    }

    /// Returns the valid DA1 feature codes in report order.
    pub fn features(&self) -> &[Feature] {
        &self.features[..self.num_features]
    }

    /// Replaces the DA1 feature list, preserving its order and duplicates.
    ///
    /// Returns [`Error::InvalidValue`] without modifying the response if more
    /// than 64 features are supplied.
    pub fn set_features(&mut self, features: &[Feature]) -> Result<()> {
        if features.len() > Self::MAX_FEATURES {
            return Err(Error::InvalidValue);
        }
        self.features[..features.len()].copy_from_slice(features);
        self.num_features = features.len();
        Ok(())
    }

    pub(crate) fn to_raw(self) -> sys::GhosttyDeviceAttributesPrimary {
        sys::GhosttyDeviceAttributesPrimary {
            conformance_level: self.conformance_level.0,
            features: self.features.map(|feature| feature.0),
            num_features: self.num_features,
        }
    }
}

/// Secondary device attributes, answering `CSI > c` (DA2).
///
/// Encoded as `CSI > device_type ; firmware_version ; rom_cartridge c`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceAttributesSecondary {
    /// Terminal type (`Pp`), for example [`DeviceType::VT220`].
    pub device_type: DeviceType,
    /// Firmware or patch version (`Pv`).
    pub firmware_version: u16,
    /// ROM cartridge registration (`Pc`); emulators conventionally report zero.
    pub rom_cartridge: u16,
}

impl DeviceAttributesSecondary {
    pub(crate) const fn to_raw(self) -> sys::GhosttyDeviceAttributesSecondary {
        sys::GhosttyDeviceAttributesSecondary {
            device_type: self.device_type.0,
            firmware_version: self.firmware_version,
            rom_cartridge: self.rom_cartridge,
        }
    }
}

/// Tertiary device attributes, answering `CSI = c` (DA3 / DECRPTUI).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceAttributesTertiary {
    /// Unit ID, encoded as eight uppercase hexadecimal digits in `DCS ! | … ST`.
    pub unit_id: u32,
}

impl DeviceAttributesTertiary {
    pub(crate) const fn to_raw(self) -> sys::GhosttyDeviceAttributesTertiary {
        sys::GhosttyDeviceAttributesTertiary {
            unit_id: self.unit_id,
        }
    }
}

/// Response data for all three device attribute query levels.
///
/// Fill all three responses in a device attributes callback; the terminal uses
/// the one matching the query. Default fields are all zero, as in the C
/// callback's initial output, rather than the library's built-in response.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DeviceAttributes {
    /// Response to a primary (`CSI c`) query.
    pub primary: DeviceAttributesPrimary,
    /// Response to a secondary (`CSI > c`) query.
    pub secondary: DeviceAttributesSecondary,
    /// Response to a tertiary (`CSI = c`) query.
    pub tertiary: DeviceAttributesTertiary,
}

impl DeviceAttributes {
    pub(crate) fn to_raw(self) -> sys::GhosttyDeviceAttributes {
        sys::GhosttyDeviceAttributes {
            primary: self.primary.to_raw(),
            secondary: self.secondary.to_raw(),
            tertiary: self.tertiary.to_raw(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::{
        ConformanceLevel, DeviceAttributes, DeviceAttributesPrimary, DeviceAttributesSecondary,
        DeviceAttributesTertiary, DeviceType, Feature,
    };
    use crate::Error;
    use crate::terminal::Terminal;

    fn query(attributes: DeviceAttributes, request: &[u8]) -> Vec<u8> {
        let output = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&output);
        let mut terminal = Terminal::new(80, 24).unwrap();
        terminal.on_write_pty(move |bytes| captured.lock().unwrap().extend_from_slice(bytes));
        terminal.on_device_attributes(move || Some(attributes));
        terminal.vt_write(request);
        output.lock().unwrap().clone()
    }

    fn c_example_attributes() -> DeviceAttributes {
        DeviceAttributes {
            primary: DeviceAttributesPrimary::new(
                ConformanceLevel::VT220,
                &[Feature::ANSI_COLOR, Feature::CLIPBOARD],
            )
            .unwrap(),
            secondary: DeviceAttributesSecondary {
                device_type: DeviceType::VT220,
                ..DeviceAttributesSecondary::default()
            },
            tertiary: DeviceAttributesTertiary::default(),
        }
    }

    #[test]
    fn device_reports_match_c_callback_example() {
        let attributes = c_example_attributes();
        assert_eq!(query(attributes, b"\x1b[c"), b"\x1b[?62;22;52c");
        assert_eq!(query(attributes, b"\x1b[>c"), b"\x1b[>1;0;0c");
        assert_eq!(query(attributes, b"\x1b[=c"), b"\x1bP!|00000000\x1b\\");
    }

    #[test]
    fn reports_custom_versions_unit_id_and_feature_codes() {
        let attributes = DeviceAttributes {
            primary: DeviceAttributesPrimary::new(
                ConformanceLevel::VT420,
                &[
                    Feature::COLUMNS_132,
                    Feature::SELECTIVE_ERASE,
                    Feature(1234),
                ],
            )
            .unwrap(),
            secondary: DeviceAttributesSecondary {
                device_type: DeviceType::VT420,
                firmware_version: 321,
                rom_cartridge: 7,
            },
            tertiary: DeviceAttributesTertiary {
                unit_id: 0xAABBCCDD,
            },
        };
        assert_eq!(query(attributes, b"\x1b[c"), b"\x1b[?64;1;6;1234c");
        assert_eq!(query(attributes, b"\x1b[>c"), b"\x1b[>41;321;7c");
        assert_eq!(query(attributes, b"\x1b[=c"), b"\x1bP!|AABBCCDD\x1b\\");
    }

    #[test]
    fn reports_all_64_feature_entries() {
        let features = core::array::from_fn::<_, 64, _>(|index| Feature(index as u16 + 1));
        let attributes = DeviceAttributes {
            primary: DeviceAttributesPrimary::new(ConformanceLevel::VT220, &features).unwrap(),
            ..DeviceAttributes::default()
        };
        let mut expected = String::from("\x1b[?62");
        for feature in features {
            use core::fmt::Write;
            write!(&mut expected, ";{}", feature.0).unwrap();
        }
        expected.push('c');
        assert_eq!(query(attributes, b"\x1b[c"), expected.as_bytes());
    }

    #[test]
    fn replacing_features_excludes_old_entries_and_rejects_overflow() {
        let mut attributes = c_example_attributes();
        attributes.primary.set_features(&[Feature::SIXEL]).unwrap();
        assert_eq!(query(attributes, b"\x1b[c"), b"\x1b[?62;4c");
        assert_eq!(
            attributes.primary.set_features(&[Feature::ANSI_COLOR; 65]),
            Err(Error::InvalidValue),
        );
        assert_eq!(query(attributes, b"\x1b[c"), b"\x1b[?62;4c");
        attributes.primary.set_features(&[]).unwrap();
        assert_eq!(query(attributes, b"\x1b[c"), b"\x1b[?62c");
    }
}
