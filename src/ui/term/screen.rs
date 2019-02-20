use std::io::Write;
use std::collections::BTreeSet;

use fnv::FnvHashMap;

use crate::utils::force_width;

#[derive(Copy, Clone, PartialEq, Eq)]
struct FmtOpts {
    w: usize,
    // `i`: The indent value.  Positive values give a hanging indent like tinyfugue, while negative
    // values give a first line indent.
    i: isize,
}

#[derive(Clone)]
struct ScreenLine {
    text: String,
    for_opts: FmtOpts,
}

fn format(text: String, opts: FmtOpts) -> Vec<ScreenLine> {
    let mut result = vec![];

    // We want to walk through the string and, so long as the amount of space it takes up so
    // far (since the last time we specified 'this should break here') is less than our view
    // width, just keep track of the last whitespace ... and keep doing this until we run out
    // of view width, where we record a break and continue on.
    //
    // We need to track our breakpoints in both characters (which we just OPTIMISTICALLY HOPE
    // will all be displayed at the same width HAHAHA) and bytes (because Rust's string slicing
    // methods all want properly aligned byte-offsets into the UTF-8 string.)  The _idx
    // variables are the byte offsets.
    let mut last_whitespace: usize = 0;
    let mut last_whitespace_idx: usize = 0;
    let mut last_breakpoint: usize = 0;
    let mut last_breakpoint_idx: usize = 0;
    let mut width_so_far: usize = 0;

    let (view_width, indent) = (opts.w, opts.i);

    let mut indent_first: String = "".to_string();
    let mut indent_rest: String = "".to_string();

    // Decide on what widths we need to wrap to so the paragraph fits properly when indented
    // according to the indent parameter.  We also build the indent strings here just to
    // not duplicate the logic.
    let indentwidth_firstline: usize = if indent < 0 {
        // Negative indents mean the first line of the paragraph is indented...
        let indent = (indent * -1) as usize;
        indent_first.push_str(&*(" ".repeat(indent)));
        view_width - indent
    } else {
        // ...and positive ones mean all the other lines are (a hanging indent, like in
        // tinyfugue.)
        view_width
    };

    // Basically the same but the other way around for the rest of the paragraph.
    let indentwidth_textbody: usize = if indent < 0 {
        view_width
    } else {
        let indent = indent as usize;
        indent_rest.push_str(&*(" ".repeat(indent)));
        view_width - indent
    };

    // TODO: This shouldn't be iterating on 'chars' since thanks to Rust's concept of a char as
    // a Unicode scalar, sometimes several chars could take up less space on the terminal than
    // expected.
    //
    // TODO: Is there a problem if we encounter input with tab characters? PROBABLY. I think we
    // probably have to special-case that.

    for (idx, character) in text.char_indices() {
        width_so_far += 1;

        if character.is_whitespace() {
            last_whitespace = width_so_far;
            last_whitespace_idx = idx;
        }

        // The target width we need to wrap to varies depending on what the indentation value
        // is. So we have to recalculate it every time.
        // We take advantage of the fact that last_breakpoint will be 0 on the first line but
        // not on any later ones.
        let target_width = match last_breakpoint {
            0 => indentwidth_firstline,
            _ => indentwidth_textbody,
        };

        // This is a while loop and not an if because I was worried about a situation where we have
        // a spot to break on whitespace but even after doing that there might still be too much
        // text.  I suspect that might never happen, but I'm not like 100% confident and there's
        // not much to lose. 
        while width_so_far - last_breakpoint > target_width {
            // We build our line by just cloning the appropriate amount of leading
            // whitespace to start with, then pushing the line itself onto the end.
            let mut line: String = match last_breakpoint {
                0 => indent_first.clone(),
                _ => indent_rest.clone(),
            };

            // If we have a whitespace point break there, but otherwise just break right
            // where we are (in the middle of, presumably, a long word) as there are no
            // other options at that point.
            if last_whitespace > last_breakpoint {
                line.push_str(text[last_breakpoint_idx..last_whitespace_idx].trim_start());
                last_breakpoint = last_whitespace;
                last_breakpoint_idx = last_whitespace_idx;
            } else {
                line.push_str(text[last_breakpoint_idx..idx].trim_start());
                last_breakpoint = width_so_far;
                last_breakpoint_idx = idx;
            }

            result.push(ScreenLine {
                text: force_width(line, opts.w),
                for_opts: opts,
            });
        }
    }

    // We still need to push the very last line... but fortunately, we still have
    // last_breakpoint_idx and can just take whatever's left over after that point.
    let last_chunk: &str = text.split_at(last_breakpoint_idx).1.trim_start();
    if last_chunk.len() > 0 {
        // We still have to decide which of these we need, because some lines are short
        // enough that they're only pushed once, here.
        let mut last_line: String = match last_breakpoint {
            0 => indent_first.clone(),
            _ => indent_rest.clone(),
        };

        last_line.push_str(last_chunk);
        result.push(ScreenLine {
            text: force_width(last_line, opts.w),
            for_opts: opts,
        });
    }

    result
}


/// A view onto some word-wrapped lines.
pub struct WrappedView {
    h: usize,
    fmt: FmtOpts,

    // This is our 'history buffer', in ascending order -- that is, the most recent line always has
    // the highest index.  We're usually going to be going in reverse chronological order because
    // we draw up from the bottom of the view and new lines appear on the bottom of the view; it's
    // a chat program, after all.
    history: Vec<String>,

    // We store a _cache_ of the results of word-wrapping each of the history lines to our view
    // settings (stored in self.fmt) so that we're not calling the relatively expensive
    // word-wrapping function on a relatively large input every single time a new line arrives and
    // we want to redraw. This is a cache and not, directly, a view buffer, because the mapping
    // from word-wrapped lines onto 'logical' history lines changes every time the view is resized.
    //
    // We use the FnvHashMap from crates.io here because it is API compatible with the regular
    // HashMap and is said to be faster for small inputs "like integers."  (Our indexes are
    // basically the same thing as array indexes.)  We're using a hashmap in the first place
    // because a Vec<> would force us to recompute every single line every time the view was
    // resized, which would gobble up a lot of CPU time with big histories.  I'm hoping the hash
    // map cache is still better than recomputing a small subset of lines every time the view is
    // rendered in that case, but I could be wrong -- I might be prematurely optimizing here.
    cache: FnvHashMap<usize, Vec<ScreenLine>>,

    // The scroll position is stored in terms of two numbers, an index onto the history line at the
    // bottom of the view (i.e., the first one we draw before working upwards to the next and the
    // next, etc.; the most recent one visible) and a measure of how many view lines within it we
    // throw away before starting to draw.  Think of the second number as a negative index.
    position: (usize, usize),
}

impl WrappedView {
    pub fn new(w: usize, h: usize) -> WrappedView {
        WrappedView {
            h,
            fmt: FmtOpts {
                i: 4, w
            },
            history: vec![],
            cache: FnvHashMap::default(),
            position: (0,0),
        }
    }

    pub fn resize(&mut self, w: usize, h: usize) {
        self.h = h;
        self.fmt.w = w;
    }

    /// Add a line to the View.
    ///
    /// This function expects that its argument will, logically, be a single line.  If you pass it
    /// a line with `\n`, `\r` or potentially other similar control characters included, it will
    /// remove them.
    pub fn push(&mut self, mut line: String) {
        line.retain(|c| c != '\n' && c != '\r');

        let current_histlen = self.history.len();
        self.history.push(line);

        // Check if we were previously at the end of the history and if so, make sure we stay at
        // the end of the history.  Special case for when the history is empty, as there's not yet
        // anything to not be at the end of.
        if current_histlen == 0 || self.position.0 == current_histlen - 1 {
            self.position.0 = self.history.len() - 1;
            self.position.1 = 0;
        }
    }

    /// Internal function: Fetch the list of word-wrapped lines representing a single logical line,
    /// recomputing only if necessary.  Called on a history index and not a String.
    fn wrap(&mut self, line: usize) -> Option<Vec<ScreenLine>> {
        if line >= self.history.len() {
            return None;
        }

        if let Some(lines) = self.cache.get(&line) {
            if lines[0].for_opts == self.fmt {
                return Some(lines.clone());
            }
        }

        // If we got here, either it hasn't been calculated yet or we changed the format options,
        // which means we'd better recompute.
        let new_lines = format(self.history[line].clone(), self.fmt);
        self.cache.insert(line, new_lines.clone());
        Some(new_lines)
    }

    /// Return a Vec of Strings representing what should currently be drawn on screen for
    /// this view.  The Vec is guaranteed to be self.h items long (index 0 = top of view) and each
    /// String attempts to be self.fmt.w `char`s wide.
    pub fn render(&mut self) -> Vec<String> {
        let lines_wanted = self.h;
        let fmt = self.fmt;

        if self.history.len() > 0 {
            // Here we have a CONFUSING TANGLE OF ITERATORS.
            //
            // This does exactly what I want, but it's probably kind of hard to read.  In fact,
            // I've even kind of confused myself.  Sorry?

            let v: Vec<String> = (0..self.position.0+1).rev().map(|i| {
                // For every line in history, going backwards from the most recent...
                self.wrap(i).expect("wrap(i) in render()").into_iter().rev()
            }).flatten().map(|l| l.text).chain(std::iter::repeat(" ".repeat(fmt.w)))
              .take(lines_wanted).collect();

            // We needed to reverse the final iterator but take() isn't a DoubleEndedIterator.  So I
            // have to consume the Vec, reverse that iterator and collect it again.  Hopefully this
            // doesn't hurt performance too much.
            v.into_iter().rev().collect()
        } else {
            std::iter::repeat(" ".repeat(fmt.w)).take(self.h).collect()
        }
    }
}



// Note: Rust docs say std::cmp::PartialOrd is derivable and will produce a lexicographic ordering
// based on the top-to-bottom declaration order of the Struct's members.  WARNING!  DO NOT CHANGE
// ORDER OF DECLARATION OF Y AND X!
//
// (TODO: Actually impl the Ord functions so this is not an 'invisible' requirement?)

/// Some point in a 2D grid with origin at 0,0.  Ord/PartialOrd are implemented such that a list of
/// these points, when sorted, will be ordered by y-value and then by x-value, such that any runs
/// of points along a single row of the grid (e.g., with the x-value increasing and y remaining
/// constant) will occur together and in order.
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug)]
struct Point {
    y: usize,
    x: usize,
}

#[test]
fn point_order() {
    let mut points: BTreeSet<Point> = BTreeSet::new();
    points.insert(Point { x: 3, y: 5 });
    points.insert(Point { x: 4, y: 5 });
    points.insert(Point { x: 5, y: 5 });
    points.insert(Point { x: 7, y: 5 });
    points.insert(Point { x: 2, y: 5 });
    points.insert(Point { x: 1, y: 6 });

    let points_ord: Vec<Point> = points.into_iter().collect();
    assert_eq!(points_ord[0], Point {x:2,y:5});
    assert_eq!(points_ord[1], Point {x:3,y:5});
    assert_eq!(points_ord[2], Point {x:4,y:5});
    assert_eq!(points_ord[3], Point {x:5,y:5});
    assert_eq!(points_ord[4], Point {x:7,y:5});
    assert_eq!(points_ord[5], Point {x:1,y:6});
}


/// Very work-in-progress 'damage buffer' type of display.
pub struct DamageBuffer {
    points_to_draw: BTreeSet<Point>,
    redraw_all: bool,
    clear_all: bool,

    w: usize,
    h: usize,
    // This was chosen to be String not Char because some Unicode characters can take up multiple
    // chars and so why not
    buffer: Vec<String>,
}

impl DamageBuffer {
    pub fn new(w: usize, h: usize) -> DamageBuffer {
        let buffer = DamageBuffer {
            w, h,
            buffer: std::iter::repeat(" ".to_string()).take(w*h).collect(),
            points_to_draw: BTreeSet::new(),
            redraw_all: false,
            clear_all: false,
        };

        buffer
    }

    pub fn clear(&mut self) {
        self.buffer = std::iter::repeat(" ".to_string())
            .take(self.w * self.h)
            .collect();
        self.points_to_draw.clear();
        self.redraw_all = false;
        self.clear_all = true;
    }

    pub fn resize(&mut self, new_w: usize, new_h: usize) {
        self.w = new_w;
        self.h = new_h;
        self.buffer.resize(self.w * self.h, " ".to_string());
        self.redraw_all = true;
    }

    pub fn write_string(&mut self, x: usize, y: usize, what: String) {
        let mut x = x;

        for c in what.chars() {
            if x < self.w && y < self.h {
                let c = c.to_string();
                // We're indexing into a 2D grid laid out row by row in a 1D memory buffer.  So we
                // compute the 1D index by multiplying y by the row length, then adding x (the
                // offset inside that row.)
                let i = y * self.w + x;

                if c != self.buffer[i] {
                    self.buffer[i] = c;
                    self.points_to_draw.insert(Point { x, y });
                }
            }
            x += 1;
        }
    }

    pub fn redraw(&mut self, term: &mut impl Write) -> std::io::Result<()> {
        let mut last_point = Point { x:0, y:0 };
        print!("{}", termion::cursor::Goto(1,1));

        // TODO: Think about ways to refactor this, since we're doing the same thing in two places.
        // You can get an Iterator over all Points with the following:
        //
        // (0..h).map(|x| std::iter::repeat(x).zip(0..w)).flatten().map(|(x,y)| Point { x,y })
        //
        // Unfortunately, I couldn't just switch which Iterator I was using because the types
        // were incompatible.  I'd probably have to call collect() on both or something, and
        // that sounds expensive.
        //
        // I could probably make a closure then call for_each() in two places depending on the
        // branch, but that seems like it'd be slower.  I should probably try doing it anyway.

        if self.clear_all {
            term.write(format!("{}", termion::clear::All).as_bytes())?;
        }

        if self.redraw_all {
            for y in 0..self.h {
                for x in 0..self.w {
                    if y != last_point.y || x as isize - last_point.x as isize != 1 {
                        term.write(format!("{}", termion::cursor::Goto((x+1) as u16, (y+1) as u16)).as_bytes())?;
                    }

                    term.write(format!("{}", self.buffer[y * self.w + x]).as_bytes())?;
                    last_point.x = x; last_point.y = y;
                }
            }
        } else {
            // See, we do the exact same thing here, just with a different source of x/y coordinates.
            for Point { x, y } in &self.points_to_draw {
                // If we have a sequence of points to write each of which is exactly one cell to
                // the right of the previous one, we can just write them out without jumping.  If
                // we *aren't* exactly one cell to the right of whatever we drew last, we jump.
                if *y != last_point.y || *x as isize - last_point.x as isize != 1 {
                    term.write(format!("{}", termion::cursor::Goto((x+1) as u16, (y+1) as u16)).as_bytes())?;
                }

                term.write(format!("{}", self.buffer[y * self.w + x]).as_bytes())?;
                last_point.x = *x; last_point.y = *y;
            }
        }

        self.points_to_draw.clear();
        self.redraw_all = false;
        self.clear_all = false;

        term.flush()
    }
}

