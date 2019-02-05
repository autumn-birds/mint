
#[derive(Copy, Clone)]
struct FmtOpts {
    w: usize,
    // `i`: The indent value.  Positive values give a hanging indent like tinyfugue, while negative
    // values give a first line indent.
    i: isize,
}

struct ScreenLine {
    text: String,
    for_opts: FmtOpts,

    // We need to keep an index around so we never forget where we are when e.g. resizing the
    // window: this points back to the original or 'logical' line we're creating a word-wrapped
    // view onto.
    from_logical: usize,
}

fn format(text: String, logical_idx: usize, opts: FmtOpts) -> Vec<ScreenLine> {
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
                text: line,
                from_logical: logical_idx,
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
            text: last_line,
            from_logical: logical_idx,
            for_opts: opts,
        });
    }

    result
}

