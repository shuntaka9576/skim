// A buffered screen that's built on top of termion which handles buffering.
// based on https://github.com/cessen/led/blob/master/src/term_ui/screen.rs

use std;
use std::cell::RefCell;
use std::io::{stdin, stdout, Write};
use std::cmp::{max, min};
use termion;
use termion::screen::AlternateScreen;
use termion::raw::IntoRawMode;
use std::os::unix::io::{IntoRawFd, RawFd};
use types::Margin;
use std::fs::OpenOptions;
use libc;

#[derive(Clone, Copy)]
enum Color {
    RED,
    BLUE,
}

#[derive(Clone, Copy)]
pub struct Style(pub Color, pub Color); // Fg, Bg

// A screen object is an abstraction of the screen to be draw on
// |
// |
// |
// +------------+ start_line
// |  ^         |
// | <          | <-- top = start_line + margin_top
// |  (margins) |
// |           >| <-- bottom = end_line - margin_bottom
// |          v |
// +------------+ end_line
// |
// |
// From the user's point of view, Screen will expose only the area within the margin
//
pub struct Screen {
    term: RefCell<Box<Write>>,
    buf: Vec<Option<(Style, char)>>,

    max_y: u16,
    max_x: u16,

    current_x: u16,
    current_y: u16,

    y_start: u16,
    top: u16,
    bottom: u16,
    left: u16,
    right: u16,

    // These information are needed for resize

    y_offset: i32, // +3 means 3 lines from top, -3 means 3 lines from bottom,
    height: Margin,
    min_height: u16,

    margin_top: Margin,
    margin_bottom: Margin,
    margin_left: Margin,
    margin_right: Margin,

    // other stuff
    orig_stdout_fd: Option<RawFd>,
}


impl Screen {
    pub fn new(height: Margin,
               min_height: u16,
               margin_top: Margin,
               margin_bottom: Margin,
               margin_left: Margin,
               margin_right: Margin,
               ) -> Self {
        // If skim is invoked by pipeline `echo 'abc' | sk | awk ...`
        // The the output is redirected. We need to open /dev/tty for output.
        let istty = unsafe { libc::isatty(libc::STDOUT_FILENO as i32) } != 0;
        let orig_stdout_fd = if !istty {
            unsafe {
                let stdout_fd = libc::dup(libc::STDOUT_FILENO);
                let tty = OpenOptions::new()
                    .write(true)
                    .open("/dev/tty")
                    .expect("curses:new: failed to open /dev/tty");
                libc::dup2(tty.into_raw_fd(), libc::STDOUT_FILENO);
                Some(stdout_fd)
            }
        } else {
            None
        };

        let (max_y, max_x) = terminal_size();

        let (term, y, x): (Box<Write>, u16, u16) = if Margin::Percent(100) == height {
            (
                Box::new(AlternateScreen::from(
                    stdout()
                        .into_raw_mode()
                        .expect("failed to set terminal to raw mode"),
                )),
                0,
                0,
            )
        } else {
            let term = Box::new(
                stdout()
                    .into_raw_mode()
                    .expect("failed to set terminal to raw mode"),
            );
            let (y, x) = get_cursor_pos();

            // reserve the necessary lines to show skim (in case current cursor is at the bottom
            // of the screen)
            reserve_lines(max_y, height, min_height);
            (term, y, x)
        };

        // keep the start position on the screen
        let y_offset = if height == Margin::Percent(100) {
            0
        } else {
            let height = match height {
                Margin::Percent(p) => max(p * max_y / 100, min_height),
                Margin::Fixed(rows) => rows,
            };
            if y + height >= max_y {
                -i32::from(height)
            } else {
                i32::from(y)
            }
        };

        let mut ret = Screen {
            term: RefCell::new(term),
            buf: Vec::new(),
            max_y,
            max_x,
            current_y: y,
            current_x: x,
            y_start: 0,
            top: 0,
            bottom: 0,
            left: 0,
            right: 0,
            y_offset,
            height,
            min_height,
            margin_top,
            margin_bottom,
            margin_left,
            margin_right,
            orig_stdout_fd,
        };
        ret.resize();
        return ret;
    }

    pub fn get_maxyx(&self) -> (u16, u16) {
        assert!(self.bottom >= self.top);
        assert!(self.right >= self.left);
        (self.bottom - self.top, self.right - self.left)
    }

    pub fn get_yx(&self) -> (u16, u16) {
        (self.current_y - self.top, self.current_x - self.left)
    }

    #[cfg_attr(rustfmt, rustfmt_skip)]
    pub fn resize(&mut self) {
        let (max_y, max_x) = terminal_size();
        self.max_y = max_y;
        self.max_x = max_x;

        let height = self.actual_height();

        let y_start = if self.y_offset >= 0 {
            self.y_offset
        } else {
            i32::from(max_y) + self.y_offset
        };

        let y_start = min(max_y-height, max(0, y_start as u16));
        self.y_start = y_start;

        self.top = y_start + match self.margin_top {
            Margin::Fixed(num) => num,
            Margin::Percent(per) => per * height / 100,
        };

        self.bottom = y_start + height - match self.margin_bottom {
            Margin::Fixed(num) => num,
            Margin::Percent(per) => per * height / 100,
        };

        self.left = match self.margin_left {
            Margin::Fixed(num) => num,
            Margin::Percent(per) => per * max_x / 100,
        };

        self.right = max_x - match self.margin_right {
            Margin::Fixed(num) => num,
            Margin::Percent(per) => per * max_x / 100,
        };

        debug!("screen:resize, TRBL: {}/{}/{}/{}", self.top, self.right, self.bottom, self.left);
        let (buf_height, buf_width) = self.get_maxyx();

        // reset buffer, normall resize will require redraw, thus we only erase anything here.
        // TODO: replace None with default Style with space
        self.buf = std::iter::repeat(None)
            .take(buf_height as usize * buf_width as usize)
            .collect();
    }

    pub fn present(&self) {
        let mut term = self.term.borrow_mut();
        let (inner_height, inner_width) = self.get_maxyx();

        // Goto the first line
        write!(term, "{}", termion::cursor::Goto(1, self.y_start + 1)).unwrap();

        // clear the top margin
        for row in self.y_start..self.top {
            write!(term, "{}", termion::cursor::Goto(1, row)).unwrap();
            write!(term, "{}", termion::clear::CurrentLine).unwrap();
        }

        for row in self.top..self.bottom {
            write!(term, "{}", termion::cursor::Goto(self.left, row)).unwrap();
            // clear the left margin
            write!(term, "{}", termion::clear::BeforeCursor).unwrap();

            // write the content
            for col in self.left..self.right {
                let inner_x = col - self.left;
                let inner_y = row - self.top;
                let index = (inner_y * inner_width + inner_x) as usize;

                if let Some((style, ch)) = self.buf[index] {
                    write!(term, "{}", ch).unwrap();
                }
            }

            // clear the right margin
            write!(term, "{}", termion::clear::AfterCursor).unwrap();
        }

        // clear the bottom margin
        for row in self.bottom..self.max_y {
            write!(term, "{}", termion::cursor::Goto(1, row)).unwrap();
            write!(term, "{}", termion::clear::CurrentLine).unwrap();
        }

        // Make sure everything is written out
        term.flush().expect("screen:present: unable to flush to termimal");
    }

    fn actual_height(&self) -> u16 {
        let (max_y, _) = terminal_size();
        match self.height {
            Margin::Percent(100) => max_y,
            Margin::Percent(p) => min(max_y, max(p * max_y / 100, self.min_height)),
            Margin::Fixed(rows) => min(max_y, rows),
        }
    }

}

fn terminal_size() -> (u16, u16) {
    termion::terminal_size().expect("curses:terminal_size: failed to get terminal size")
}

fn get_cursor_pos() -> (u16, u16) {
    let mut stdout = stdout()
        .into_raw_mode()
        .expect("screen:get_cursor_pos: failed to set stdout to raw mode");

    let mut f = stdin();
    write!(stdout, "\x1B[6n").expect("screen:get_cursor_pos: failed to write to stdout");

    stdout
        .flush()
        .expect("screen:get_cursor_pos: failed to flush stdout");

    let mut chars = Vec::new();
    loop {
        let mut buf = [0; 1];
        let _ = f.read(&mut buf);
        chars.push(buf[0]);
        if buf[0] == b'R' {
            break;
        }
    }

    let s = String::from_utf8(chars).expect("screen:get_cursor_pos: invalid utf8 string read");
    let t: Vec<&str> = s[2..s.len() - 1].split(';').collect();
    stdout.flush().expect("screen:get_cursor_pos: failed to flush stdout");

    let y = t[0].parse::<u16>().expect("screen:get_cursor_pos: invalid position y");
    let x = t[1].parse::<u16>().expect("screen:get_cursor_pos: invalid position x");

    (y - 1, x - 1)
}

fn reserve_lines(height: u16, reserved_height: Margin, min_height: u16) {
    let rows = match reserved_height {
        Margin::Percent(100) => {
            return;
        }
        Margin::Percent(percent) => max(min_height, height * percent / 100),
        Margin::Fixed(rows) => rows,
    };

    print!("{}", "\n".repeat(max(0, rows - 1) as usize));
    stdout().flush().expect("screen:reserve_lines: failed to write to stdout");
}
