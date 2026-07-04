//! Tiny fuzzy subsequence matcher used by the command palette and pickers.
//! Not as clever as `nucleo`, but dependency-free and good enough for short
//! action/label lists.

/// Score `haystack` against `needle`. Returns `None` when `needle` is not a
/// subsequence of `haystack` (case-insensitive). Higher scores are better.
pub fn score(needle: &str, haystack: &str) -> Option<i64> {
    if needle.is_empty() {
        return Some(0);
    }
    let hay: Vec<char> = haystack.to_lowercase().chars().collect();
    let nee: Vec<char> = needle.to_lowercase().chars().collect();

    let mut hi = 0usize;
    let mut ni = 0usize;
    let mut total = 0i64;
    let mut streak = 0i64;
    let mut first_match: Option<usize> = None;

    while hi < hay.len() && ni < nee.len() {
        if hay[hi] == nee[ni] {
            if first_match.is_none() {
                first_match = Some(hi);
            }
            streak += 1;
            // Reward contiguous matches and word-boundary matches.
            total += 1 + streak;
            if hi == 0 || !hay[hi - 1].is_alphanumeric() {
                total += 3;
            }
            ni += 1;
        } else {
            streak = 0;
        }
        hi += 1;
    }

    if ni == nee.len() {
        // Penalise later first-match positions and longer haystacks slightly.
        let penalty = first_match.unwrap_or(0) as i64 + (hay.len() as i64) / 8;
        Some(total * 4 - penalty)
    } else {
        None
    }
}

/// Filter and rank `items` by their fuzzy score against `query`, best first.
/// Returns the original indices of the matching items.
pub fn rank<'a, I, S>(query: &str, items: I) -> Vec<usize>
where
    I: IntoIterator<Item = &'a S>,
    S: AsRef<str> + 'a,
{
    let mut scored: Vec<(usize, i64)> = items
        .into_iter()
        .enumerate()
        .filter_map(|(i, s)| score(query, s.as_ref()).map(|sc| (i, sc)))
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    scored.into_iter().map(|(i, _)| i).collect()
}
