/// Input keys handled by the [`Viewer`] state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// Scroll up by one line.
    Up,
    /// Scroll down by one line.
    Down,
    /// Scroll up by one page.
    PageUp,
    /// Scroll down by one page.
    PageDown,
    /// Jump to the beginning of the file (`offset = 0`).
    Home,
    /// Jump to the end of the file.
    End,
    /// Toggle live follow mode on or off.
    ToggleFollow,
    /// Exit the viewer.
    Quit,
}

/// Side-effect free action emitted by [`Viewer::handle`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Scroll backward by the given number of lines.
    ScrollUp(usize),
    /// Scroll forward by the given number of lines.
    ScrollDown(usize),
    /// Jump to the start of the log source.
    JumpStart,
    /// Jump to the end of the log source.
    JumpEnd,
    /// Follow mode was toggled to the contained boolean state.
    SetFollow(bool),
    /// Quit the viewer loop.
    Quit,
}

/// Pure state machine for interactive log navigation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Viewer {
    page_lines: usize,
    follow: bool,
    quit: bool,
}

impl Viewer {
    /// Creates a new [`Viewer`] state machine with the given viewport page height (in lines).
    #[must_use]
    pub fn new(page_lines: usize) -> Self {
        Self {
            page_lines: page_lines.max(1),
            follow: false,
            quit: false,
        }
    }

    /// Returns `true` if follow mode is currently enabled.
    #[must_use]
    pub const fn follow(&self) -> bool {
        self.follow
    }

    /// Returns `true` if the viewer has received [`Key::Quit`].
    #[must_use]
    pub const fn is_quit(&self) -> bool {
        self.quit
    }

    /// Returns the configured page size in lines.
    #[must_use]
    pub const fn page_lines(&self) -> usize {
        self.page_lines
    }

    /// Processes a [`Key`] event, updating internal state and returning the corresponding [`Action`].
    pub fn handle(&mut self, key: Key) -> Action {
        match key {
            Key::Up => {
                self.follow = false;
                Action::ScrollUp(1)
            }
            Key::Down => Action::ScrollDown(1),
            Key::PageUp => {
                self.follow = false;
                Action::ScrollUp(self.page_lines)
            }
            Key::PageDown => Action::ScrollDown(self.page_lines),
            Key::Home => {
                self.follow = false;
                Action::JumpStart
            }
            Key::End => Action::JumpEnd,
            Key::ToggleFollow => {
                self.follow = !self.follow;
                Action::SetFollow(self.follow)
            }
            Key::Quit => {
                self.quit = true;
                Action::Quit
            }
        }
    }
}

impl Default for Viewer {
    fn default() -> Self {
        Self::new(20)
    }
}
