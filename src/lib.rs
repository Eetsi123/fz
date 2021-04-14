use crossterm::{
    cursor, event,
    event::{Event, KeyCode, KeyEvent, KeyModifiers},
    terminal,
    terminal::{
        ClearType, DisableLineWrap, EnableLineWrap, EnterAlternateScreen, LeaveAlternateScreen,
    },
    ErrorKind as CrosstermError, ExecutableCommand, QueueableCommand,
};
use fuzzy_matcher::skim::SkimMatcherV2;

use std::{
    borrow::Cow,
    fmt,
    fmt::{Display, Formatter},
    io::{Error as IoError, Write},
    slice,
    time::Duration,
};

macro_rules! impl_error {
    ($($err:ident),*) => {
        #[derive(Debug)]
        pub enum Error {
            $($err($err)),*
        }

        impl std::error::Error for Error {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                match self {
                    $(Self::$err(e) => Some(e)),*
                }
            }
        }

        impl Display for Error {
            fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
                match self {
                    $(Self::$err(e) => write!(f, "{}", e)),*
                }
            }
        }

        $(
            impl From<$err> for Error {
                fn from(e: $err) -> Self { Self::$err(e) }
            }
        )*
    }
}

pub fn select<'a, W: Write>(writer: W, list: &'a [&str]) -> Result<Cow<'a, [&'a str]>> {
    Fz::new(writer)?.select(list)
}

pub type Result<T> = std::result::Result<T, Error>;
impl_error!(IoError, CrosstermError);

struct Fz<'a, W: Write> {
    pattern: String,        // pattern written by user
    matches: Vec<&'a str>,  // items matched by the pattern
    offset: usize,          // offset of first item shown to user
    index: usize,           // visible position, upwards from the bottom
    selected: Vec<&'a str>, // selected items
    writer: W,              // stdout/stderr
    width: u16,             // height of terminal
    height: u16,            // width of terminal
}

impl<'a, W: Write> Fz<'a, W> {
    fn new(writer: W) -> Result<Self> {
        let (width, height) = terminal::size()?;
        Ok(Self {
            pattern: String::new(),
            matches: Vec::new(),
            offset: 0,
            index: 0,
            selected: Vec::new(),
            writer,
            width,
            height,
        })
    }

    #[inline]
    fn max_rows(&self) -> u16 {
        self.height - 2
    }

    fn move_cursor(&self) -> cursor::MoveTo {
        // move cursor to the last line, to the end of pattern
        cursor::MoveTo(self.pattern.chars().count() as u16, self.height - 1)
    }

    fn select(mut self, list: &'a [&str]) -> Result<Cow<'a, [&'a str]>> {
        // initially fill matches with the whole list
        self.update_matches(list);

        // setup
        terminal::enable_raw_mode()?;
        self.writer
            .queue(EnterAlternateScreen)?
            .queue(DisableLineWrap)?;

        // initial draw
        self.redraw()?;
        self.writer.execute(self.move_cursor())?;

        // event loop
        loop {
            // poll if an event is available
            if let Ok(true) = event::poll(Duration::from_secs(2)) {
                match event::read() {
                    // handle resize
                    Ok(Event::Resize(w, h)) => {
                        self.width = w;
                        self.height = h;
                        self.redraw()?;
                    }
                    // return selected items
                    Ok(Event::Key(
                        KeyEvent {
                            code: KeyCode::Enter,
                            ..
                        }
                        | KeyEvent {
                            code: KeyCode::Char('m'),
                            modifiers: KeyModifiers::CONTROL,
                        },
                    )) => break,
                    // move up a row
                    Ok(Event::Key(
                        KeyEvent {
                            code: KeyCode::Up, ..
                        }
                        | KeyEvent {
                            code: KeyCode::Char('p'),
                            modifiers: KeyModifiers::CONTROL,
                        },
                    )) => {
                        if !self.matches.is_empty() {
                            // don't go up if there are no more matches
                            if self.offset + self.index < self.matches.len() - 1 {
                                // clear previous position marker
                                self.position(false)?;

                                match self.index == self.max_rows() as usize {
                                    // increment index
                                    false => self.index += 1,
                                    // on topmost row -> move the whole view up
                                    true => {
                                        self.offset += 1;
                                        self.redraw()?;
                                    }
                                }

                                // draw new position marker
                                self.position(true)?;
                            }
                        }
                    }
                    // move down a row
                    Ok(Event::Key(
                        KeyEvent {
                            code: KeyCode::Down,
                            ..
                        }
                        | KeyEvent {
                            code: KeyCode::Char('n'),
                            modifiers: KeyModifiers::CONTROL,
                        },
                    )) => {
                        if !self.matches.is_empty() {
                            // don't go down if already at first match
                            if self.offset + self.index > 0 {
                                // clear previous position marker
                                self.position(false)?;

                                match self.index == 0 {
                                    // decrement index
                                    false => self.index -= 1,
                                    // on bottom row -> move the whole view down
                                    true => {
                                        self.offset -= 1;
                                        self.redraw()?;
                                    }
                                }

                                // draw new position marker
                                self.position(true)?;
                            }
                        }
                    }
                    // toggle selection
                    Ok(Event::Key(KeyEvent {
                        code: KeyCode::Tab, ..
                    })) => {
                        if !self.matches.is_empty() {
                            let current_item = self.matches[self.offset + self.index];

                            // find the index of current_item in selected if it has one
                            match self.selected.iter().position(|s| *s == current_item) {
                                // remove the (existing) selection
                                Some(index) => {
                                    self.selected.remove(index);
                                    self.selection(false, self.index as u16)?;
                                }
                                // add a new selection
                                None => {
                                    self.selected.push(current_item);
                                    self.selection(true, self.index as u16)?;
                                }
                            }
                        }
                    }
                    // erase a character from pattern
                    Ok(Event::Key(KeyEvent {
                        code: KeyCode::Backspace,
                        ..
                    })) => {
                        self.pattern.pop();

                        self.update_matches(list);
                        self.redraw()?;
                    }
                    // add a character to pattern (only if no modifiers except SHIFT are pressed)
                    Ok(Event::Key(KeyEvent {
                        code: KeyCode::Char(c),
                        modifiers: km,
                    })) if !km.intersects(!KeyModifiers::SHIFT) => {
                        match km {
                            KeyModifiers::NONE => self.pattern.push(c),
                            KeyModifiers::SHIFT => self.pattern.push(c.to_ascii_uppercase()),
                            _ => unreachable!(),
                        }

                        self.update_matches(list);
                        self.redraw()?;
                    }
                    _ => (),
                }
            }

            // move cursor and flush changes
            self.writer.execute(self.move_cursor())?;
        }

        // undo the setup
        self.writer
            .queue(LeaveAlternateScreen)?
            .execute(EnableLineWrap)?;
        terminal::disable_raw_mode()?;

        // return selected items
        let selected = match self.selected.is_empty() {
            true => match self.matches.is_empty() {
                true => Cow::Borrowed(&[] as &[&str]),
                false => {
                    // borrow selected item from list to satisfy borrow checker
                    let selected_item = list
                        .iter()
                        .find(|&i| i == &self.matches[self.offset + self.index])
                        .unwrap();
                    Cow::Borrowed(slice::from_ref(selected_item))
                }
            },
            false => Cow::from(self.selected),
        };
        Ok(selected)
    }

    fn redraw(&mut self) -> Result<()> {
        // clear the whole screen
        self.writer.queue(terminal::Clear(ClearType::All))?;

        // can't change during drawing
        let max_rows = self.max_rows();

        // draw rows
        for (i, m) in self
            .matches
            .iter()
            .skip(self.offset) // start iterating matches from offset
            .take(max_rows as usize + 1) // only print matches that fit on screen
            .enumerate()
        {
            // draw the match
            self.writer
                .queue(cursor::MoveTo(2, max_rows - i as u16))?
                .write_all(m.as_bytes())?;

            // draw selection marker if the match is selected
            if self.selected.contains(m) {
                // inlined self.selection to satisfy borrow checker
                self.writer
                    .queue(cursor::MoveTo(1, max_rows - i as u16))?
                    .write_all(b"*")?;
            }

            // end overflowing lines with ..
            if m.chars().count() > self.width as usize - 2
            // matches start from third column
            {
                self.writer
                    .queue(cursor::MoveTo(self.width - 2, max_rows - i as u16))?
                    .write_all(b"..")?;
            }
        }

        if !self.matches.is_empty() {
            // draw position marker
            self.position(true)?;
        }

        // draw pattern
        self.writer
            .queue(cursor::MoveTo(0, self.height - 1))?
            .write_all(self.pattern.as_bytes())?;

        Ok(())
    }

    // shows or hides the position marker for current index
    fn position(&mut self, show: bool) -> Result<()> {
        let character = match show {
            true => b'>',
            false => b' ',
        };

        self.writer
            .queue(cursor::MoveTo(0, self.max_rows() - self.index as u16))?
            .write_all(&[character])?;

        Ok(())
    }

    // shows or hides the selection marker for given row
    fn selection(&mut self, show: bool, row: u16) -> Result<()> {
        let character = match show {
            true => b'*',
            false => b' ',
        };

        self.writer
            .queue(cursor::MoveTo(1, self.max_rows() - row))?
            .write_all(&[character])?;

        Ok(())
    }

    fn update_matches(&mut self, items: &'a [&str]) {
        self.matches.clear();

        match self.pattern.is_empty() {
            // match all items if pattern is empty
            true => {
                // add all items and sort them
                self.matches.extend(items);
                self.matches.sort_unstable();
                // there can't be less matches than previously
                //   -> offset + index will point to an existing item
            }
            // fuzzy match items with non-empty pattern
            false => {
                let matcher = SkimMatcherV2::default();
                // items with corresponding scores (for sorting)
                let mut scored = Vec::new();

                for item in items {
                    if let Some((score, _indices)) = matcher.fuzzy(item, &self.pattern, false) {
                        scored.push((item, score));
                    }
                }

                scored.sort_unstable_by(|(a_item, a_score), (b_item, b_score)| {
                    match a_score == b_score {
                        false => a_score.cmp(b_score), // sort by score
                        true => a_item.cmp(b_item),    // sort by item if scores are equal
                    }
                });

                // add sorted matches
                self.matches.extend(scored.into_iter().map(|(i, _s)| i));

                // reset offset so that matches with best scores are visible
                self.offset = 0;

                match self.matches.is_empty() {
                    // reset index back to 0 if there are no matches
                    true => self.index = 0,
                    false => {
                        if self.index >= self.matches.len() {
                            // set index to point to the last item
                            self.index = self.matches.len() - 1;
                        }
                    }
                }
            }
        }
    }
}
