/// Keeps the first `max_chars` characters: listings and search hits matter from the start.
pub fn head(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let head: String = text.chars().take(max_chars).collect();
    format!("{head}…")
}

/// Keeps the last `max_chars` characters: errors and summaries are usually at the end.
pub fn tail(text: &str, max_chars: usize) -> String {
    let len = text.chars().count();
    if len <= max_chars {
        return text.to_string();
    }
    let tail: String = text.chars().skip(len - max_chars).collect();
    format!("…{tail}")
}
