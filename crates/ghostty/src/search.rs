//! Searching terminal content.

use crate::{Error, Result, ffi, selection::Selection, terminal::Terminal};
use core::ptr::{self, NonNull};
use ghostty_sys as sys;

ffi::c_enum! {
    /// Current progress of a terminal search.
    pub enum SearchStatus {
        /// Copied data can be searched with another tick.
        Running = sys::GHOSTTY_SEARCH_STATUS_RUNNING,
        /// A feed is required to read more terminal data.
        FeedRequired = sys::GHOSTTY_SEARCH_STATUS_FEED_REQUIRED,
        /// Caught up with the terminal at the last feed.
        Complete = sys::GHOSTTY_SEARCH_STATUS_COMPLETE,
    }
}
ffi::c_enum! {
    /// Viewport policy when selecting a search match.
    pub enum SearchScroll {
        /// Scroll only when the match is not visible.
        IfNeeded = sys::GHOSTTY_SEARCH_SCROLL_IF_NEEDED,
        /// Never scroll.
        None = sys::GHOSTTY_SEARCH_SCROLL_NONE,
    }
}

/// A search exclusively borrowing its terminal.
///
/// Although C can detach a search from a freed terminal and tick copied data on
/// another thread, the exclusive borrow serializes terminal access and ensures
/// selections cannot outlive the next operation that may scroll the terminal.
pub struct Search<'t> {
    raw: NonNull<sys::GhosttySearchImpl>,
    terminal: &'t mut Terminal,
}

impl<'t> Search<'t> {
    /// Creates an idle search with no query.
    pub fn new(terminal: &'t mut Terminal) -> Result<Self> {
        let mut raw = ptr::null_mut();
        unsafe {
            Error::check(sys::ghostty_search_new(
                ptr::null(),
                &mut raw,
                terminal.as_raw(),
            ))?;
        }
        Ok(Self {
            raw: NonNull::new(raw).ok_or(Error::OutOfMemory)?,
            terminal,
        })
    }
    /// Returns the searched terminal for reading or formatting.
    pub fn terminal(&self) -> &Terminal {
        self.terminal
    }
    /// Sets a byte query (ASCII case-insensitive), or clears it with `None`.
    pub fn set_needle(&mut self, needle: Option<&[u8]>) -> Result<()> {
        let value = needle.map(ffi::ghostty_string);
        unsafe {
            Error::check(sys::ghostty_search_set(
                self.raw.as_ptr(),
                sys::GHOSTTY_SEARCH_OPT_NEEDLE,
                value.as_ref().map_or(ptr::null(), |value| {
                    (value as *const sys::GhosttyString).cast()
                }),
            ))
        }
    }
    /// Sets the viewport policy used when selecting matches.
    pub fn set_scroll(&mut self, scroll: SearchScroll) -> Result<()> {
        unsafe {
            Error::check(sys::ghostty_search_set(
                self.raw.as_ptr(),
                sys::GHOSTTY_SEARCH_OPT_SELECT_SCROLL,
                (&scroll.to_raw() as *const i32).cast(),
            ))
        }
    }
    /// Performs a bounded amount of work on copied terminal data.
    pub fn tick(&mut self) -> Result<SearchStatus> {
        let mut status = 0;
        unsafe {
            Error::check(sys::ghostty_search_tick(self.raw.as_ptr(), &mut status))?;
        }
        Ok(SearchStatus::from_lib(status))
    }
    /// Reads the next bounded chunk of terminal state.
    pub fn feed(&mut self) -> Result<()> {
        unsafe { Error::check(sys::ghostty_search_feed(self.raw.as_ptr())) }
    }
    /// Feeds and ticks until caught up with the terminal.
    pub fn run(&mut self) -> Result<()> {
        unsafe { Error::check(sys::ghostty_search_run(self.raw.as_ptr())) }
    }
    /// Selects the next older match, wrapping; false means there are no matches.
    pub fn select_next(&mut self) -> Result<bool> {
        Self::selected(unsafe { sys::ghostty_search_select_next(self.raw.as_ptr()) })
    }
    /// Selects the next newer match, wrapping; false means there are no matches.
    pub fn select_prev(&mut self) -> Result<bool> {
        Self::selected(unsafe { sys::ghostty_search_select_prev(self.raw.as_ptr()) })
    }
    fn selected(code: sys::GhosttyResult) -> Result<bool> {
        if code == sys::GHOSTTY_NO_VALUE {
            return Ok(false);
        }
        Error::check(code)?;
        Ok(true)
    }
    fn get<T: Default>(&self, data: sys::GhosttySearchData) -> Result<T> {
        let mut value = T::default();
        unsafe {
            Error::check(sys::ghostty_search_get(
                self.raw.as_ptr(),
                data,
                (&raw mut value).cast(),
            ))?;
        }
        Ok(value)
    }
    fn optional<T: Default>(&self, data: sys::GhosttySearchData) -> Result<Option<T>> {
        match self.get(data) {
            Ok(value) => Ok(Some(value)),
            Err(Error::NoValue) => Ok(None),
            Err(error) => Err(error),
        }
    }
    /// Returns the current progress state.
    pub fn status(&self) -> Result<SearchStatus> {
        self.get(sys::GHOSTTY_SEARCH_DATA_STATUS)
            .map(SearchStatus::from_lib)
    }
    /// Returns the borrowed query bytes, if any.
    pub fn needle(&self) -> Result<Option<&[u8]>> {
        // SAFETY: the returned bytes borrow self, preventing query changes or destruction.
        Ok(self
            .optional(sys::GHOSTTY_SEARCH_DATA_NEEDLE)?
            .map(|raw| unsafe { ffi::string_bytes(raw) }))
    }
    /// Returns the number of matches found on the active screen.
    pub fn total_matches(&self) -> Result<usize> {
        self.get(sys::GHOSTTY_SEARCH_DATA_TOTAL_MATCHES)
    }
    /// Returns the selected index in newest-to-oldest order.
    pub fn selected_index(&self) -> Result<Option<usize>> {
        self.optional(sys::GHOSTTY_SEARCH_DATA_SELECTED_INDEX)
    }
    /// Returns the selected match as a terminal-borrowing snapshot.
    pub fn selected_match(&self) -> Result<Option<Selection<'_>>> {
        // SAFETY: no terminal mutation is possible while self is borrowed.
        Ok(self
            .optional(sys::GHOSTTY_SEARCH_DATA_SELECTED_MATCH)?
            .map(|raw| unsafe { Selection::from_raw(self.terminal, &raw) }))
    }
    /// Returns the configured viewport policy.
    pub fn scroll(&self) -> Result<SearchScroll> {
        self.get(sys::GHOSTTY_SEARCH_DATA_SELECT_SCROLL)
            .map(SearchScroll::from_lib)
    }
    fn selections(&self, data: sys::GhosttySearchData) -> Result<Vec<Selection<'_>>> {
        let values = ffi::encode_vec(|buffer, capacity, written| {
            let mut buffer = sys::GhosttySelectionBuffer {
                ptr: buffer,
                cap: capacity,
                len: 0,
            };
            let code = unsafe {
                sys::ghostty_search_get(self.raw.as_ptr(), data, (&raw mut buffer).cast())
            };
            *written = buffer.len;
            code
        })?;
        // SAFETY: all snapshots come from this terminal, and borrowing self prevents mutation.
        Ok(values
            .iter()
            .map(|raw| unsafe { Selection::from_raw(self.terminal, raw) })
            .collect())
    }
    /// Returns all matches in newest-to-oldest order.
    pub fn matches(&self) -> Result<Vec<Selection<'_>>> {
        self.selections(sys::GHOSTTY_SEARCH_DATA_MATCHES)
    }
    /// Returns cached matches on pages covering the last-fed viewport.
    pub fn viewport_matches(&self) -> Result<Vec<Selection<'_>>> {
        self.selections(sys::GHOSTTY_SEARCH_DATA_VIEWPORT_MATCHES)
    }
}
impl Drop for Search<'_> {
    fn drop(&mut self) {
        // SAFETY: exclusive terminal ownership serializes release of tracked state.
        unsafe { sys::ghostty_search_free(self.raw.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formatter::Formatter;
    #[test]
    fn matches_and_next_previous() -> Result<()> {
        let mut terminal = Terminal::new(40, 5)?;
        terminal.vt_write(b"hello world\r\nHELLO again\r\nlast hello");
        let mut search = Search::new(&mut terminal)?;
        search.set_needle(Some(b"hello"))?;
        assert_eq!(search.status()?, SearchStatus::FeedRequired);
        search.run()?;
        assert_eq!(search.total_matches()?, 3);
        assert_eq!(search.matches()?.len(), 3);
        assert_eq!(search.viewport_matches()?.len(), 3);
        assert!(search.select_next()?);
        let first = search.selected_index()?.unwrap();
        {
            let selection = search.selected_match()?.unwrap();
            let mut formatter = Formatter::new(search.terminal())?;
            formatter.set_selection(Some(&selection))?;
            assert_eq!(formatter.format_to_vec()?, b"hello");
        }
        assert!(search.select_next()?);
        assert_eq!(search.selected_index()?, Some((first + 1) % 3));
        assert!(search.select_prev()?);
        assert_eq!(search.selected_index()?, Some(first));
        search.set_needle(None)?;
        assert_eq!(search.total_matches()?, 0);
        assert!(!search.select_next()?);
        Ok(())
    }
    #[test]
    fn bounded_feed_and_tick() -> Result<()> {
        let mut terminal = Terminal::new(20, 2)?;
        for _ in 0..100 {
            terminal.vt_write(b"match\r\n");
        }
        let mut search = Search::new(&mut terminal)?;
        search.set_needle(Some(b"match"))?;
        loop {
            match search.tick()? {
                SearchStatus::Running => (),
                SearchStatus::FeedRequired => search.feed()?,
                SearchStatus::Complete => break,
            }
        }
        assert_eq!(search.total_matches()?, 100);
        Ok(())
    }
}
