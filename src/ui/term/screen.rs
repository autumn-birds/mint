use std::io::Write;
use std::collections::BTreeSet;

// also uses termion. TODO: Import at top level of term module? Would that even work?

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

