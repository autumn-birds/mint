
/// Return a version of `text` that is exactly `width` chars long.  Truncates if it is too long,
/// and appends space characters if it is not long enough.
pub fn force_width(mut text: String, width: usize) -> String {
    // TODO: Do this in a less stupid way...
    while text.len() > width {
        text.pop();
    }

    while text.len() < width {
        text.push(' ');
    }

    text
}

