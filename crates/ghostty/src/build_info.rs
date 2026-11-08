//! Build options, ABI manifest, and version of the linked libghostty-vt.

use std::{ffi::CStr, sync::LazyLock};

use ghostty_sys as sys;

use crate::ffi::c_enum;
use crate::{Error, Result, ffi};

/// The header version, without prerelease or build metadata.
pub const HEADER_VERSION: &str = match sys::GHOSTTY_VT_VERSION.to_str() {
    Ok(version) => version,
    Err(_) => panic!("libghostty-vt header version is not UTF-8"),
};
/// The major version of the headers used to compile this crate.
pub const HEADER_VERSION_MAJOR: u32 = sys::GHOSTTY_VT_VERSION_MAJOR;
/// The minor version of the headers used to compile this crate.
pub const HEADER_VERSION_MINOR: u32 = sys::GHOSTTY_VT_VERSION_MINOR;
/// The patch version of the headers used to compile this crate.
pub const HEADER_VERSION_PATCH: u32 = sys::GHOSTTY_VT_VERSION_PATCH;

c_enum! {
    /// The optimization mode used to build the linked library.
    pub enum OptimizeMode {
        /// Debug build.
        Debug = sys::GHOSTTY_OPTIMIZE_DEBUG,
        /// Optimized with runtime safety checks.
        ReleaseSafe = sys::GHOSTTY_OPTIMIZE_RELEASE_SAFE,
        /// Optimized for code size.
        ReleaseSmall = sys::GHOSTTY_OPTIMIZE_RELEASE_SMALL,
        /// Optimized for speed.
        ReleaseFast = sys::GHOSTTY_OPTIMIZE_RELEASE_FAST,
    }
}

/// Returns the full library version, including prerelease and build metadata.
pub fn version() -> &'static str {
    // SAFETY: the library guarantees a nonnull, immutable, NUL-terminated
    // version string with process lifetime; the library is statically linked.
    unsafe { CStr::from_ptr(sys::ghostty_version()) }
        .to_str()
        .expect("libghostty-vt version is not UTF-8")
}

/// Returns the linked build's process-lifetime JSON ABI manifest.
pub fn type_json() -> &'static str {
    // SAFETY: ghostty_type_json returns a NUL-terminated process-lifetime JSON
    // string, and the statically linked library cannot be unloaded.
    unsafe { CStr::from_ptr(sys::ghostty_type_json()) }
        .to_str()
        .expect("libghostty-vt ABI manifest is not UTF-8")
}

/// The linked library's compile-time configuration, queried once per process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildInfo {
    /// SIMD-accelerated code paths are enabled.
    pub simd: bool,
    /// Kitty graphics support is included.
    pub kitty_graphics: bool,
    /// Tmux control mode support is included.
    pub tmux_control_mode: bool,
    /// Build optimization mode.
    pub optimize: OptimizeMode,
    /// Full version string, including prerelease and build metadata.
    pub version_string: &'static str,
    /// Major version number.
    pub version_major: usize,
    /// Minor version number.
    pub version_minor: usize,
    /// Patch version number.
    pub version_patch: usize,
    /// Prerelease metadata, or an empty string.
    pub version_pre: &'static str,
    /// Build metadata, or an empty string.
    pub version_build: &'static str,
    /// Snapshot support is included.
    pub snapshot: bool,
    /// Formatting support is included.
    pub formatter: bool,
    /// Selection support is included.
    pub selection: bool,
    /// Search support is included.
    pub search: bool,
    /// Render-state support is included.
    pub render_state: bool,
    /// Input encoding support is included.
    pub input_encode: bool,
    /// Color parsing support is included.
    pub color: bool,
    /// Grid introspection support is included.
    pub grid_introspection: bool,
    /// Custom glyph protocol support is included.
    pub glyph_protocol: bool,
}

impl BuildInfo {
    /// Queries all build fields once and returns their process-lifetime value.
    pub fn get() -> Result<&'static Self> {
        static INFO: LazyLock<Result<BuildInfo>> = LazyLock::new(BuildInfo::query);
        INFO.as_ref().map_err(|error| *error)
    }

    fn query() -> Result<Self> {
        // SAFETY: each key below is paired with exactly the output type
        // documented by GhosttyBuildInfo. Strings are immutable process-lifetime
        // build constants; linking statically keeps them alive permanently.
        unsafe {
            Ok(Self {
                simd: query(sys::GHOSTTY_BUILD_INFO_SIMD)?,
                kitty_graphics: query(sys::GHOSTTY_BUILD_INFO_KITTY_GRAPHICS)?,
                tmux_control_mode: query(sys::GHOSTTY_BUILD_INFO_TMUX_CONTROL_MODE)?,
                optimize: OptimizeMode::from_lib(query(sys::GHOSTTY_BUILD_INFO_OPTIMIZE)?),
                version_string: query_string(sys::GHOSTTY_BUILD_INFO_VERSION_STRING)?,
                version_major: query(sys::GHOSTTY_BUILD_INFO_VERSION_MAJOR)?,
                version_minor: query(sys::GHOSTTY_BUILD_INFO_VERSION_MINOR)?,
                version_patch: query(sys::GHOSTTY_BUILD_INFO_VERSION_PATCH)?,
                version_pre: query_string(sys::GHOSTTY_BUILD_INFO_VERSION_PRE)?,
                version_build: query_string(sys::GHOSTTY_BUILD_INFO_VERSION_BUILD)?,
                snapshot: query(sys::GHOSTTY_BUILD_INFO_FEATURE_SNAPSHOT)?,
                formatter: query(sys::GHOSTTY_BUILD_INFO_FEATURE_FORMATTER)?,
                selection: query(sys::GHOSTTY_BUILD_INFO_FEATURE_SELECTION)?,
                search: query(sys::GHOSTTY_BUILD_INFO_FEATURE_SEARCH)?,
                render_state: query(sys::GHOSTTY_BUILD_INFO_FEATURE_RENDER_STATE)?,
                input_encode: query(sys::GHOSTTY_BUILD_INFO_FEATURE_INPUT_ENCODE)?,
                color: query(sys::GHOSTTY_BUILD_INFO_FEATURE_COLOR)?,
                grid_introspection: query(sys::GHOSTTY_BUILD_INFO_FEATURE_GRID_INTROSPECTION)?,
                glyph_protocol: query(sys::GHOSTTY_BUILD_INFO_FEATURE_GLYPH_PROTOCOL)?,
            })
        }
    }
}

// SAFETY contract: `key` must specify output type T.
unsafe fn query<T: Default>(key: sys::GhosttyBuildInfo) -> Result<T> {
    let mut value = T::default();
    unsafe {
        Error::check(sys::ghostty_build_info(key, (&raw mut value).cast()))?;
    }
    Ok(value)
}

// SAFETY contract: `key` must specify a process-lifetime GhosttyString.
unsafe fn query_string(key: sys::GhosttyBuildInfo) -> Result<&'static str> {
    // SAFETY: the caller chooses a static GhosttyString key.
    let bytes = unsafe { ffi::string_bytes(query::<sys::GhosttyString>(key)?) };
    Ok(std::str::from_utf8(bytes).expect("libghostty-vt version metadata is not UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_version_and_build_metadata_match_headers() {
        let info = BuildInfo::get().unwrap();
        assert_eq!(info.version_string, version());
        assert_eq!(info.version_major, HEADER_VERSION_MAJOR as usize);
        assert_eq!(info.version_minor, HEADER_VERSION_MINOR as usize);
        assert_eq!(info.version_patch, HEADER_VERSION_PATCH as usize);
        let mut expected = format!(
            "{}.{}.{}",
            info.version_major, info.version_minor, info.version_patch
        );
        assert_eq!(expected, HEADER_VERSION);
        if !info.version_pre.is_empty() {
            expected.push('-');
            expected.push_str(info.version_pre);
        }
        if !info.version_build.is_empty() {
            expected.push('+');
            expected.push_str(info.version_build);
        }
        assert_eq!(version(), expected);
    }
}
