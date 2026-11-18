const SEPARATORS: &str = "/-_. ";

fn token_score(token: &[char], text: &[char]) -> Option<i64> {
    let is_sep = |i: usize| i == 0 || SEPARATORS.contains(text[i - 1]);
    if let Some(i) = text.windows(token.len()).position(|w| w == token) {
        let mut score = 100 + 10 * token.len() as i64;
        if is_sep(i) {
            score += 50;
        }
        if text.iter().rposition(|&c| c == '/').is_none_or(|slash| i > slash) {
            score += 40;
        }
        return Some(score);
    }
    // Fall back to an in-order subsequence match, like fzf.
    let (mut score, mut start, mut prev) = (0, 0, usize::MAX - 1);
    for ch in token {
        let pos = start + text[start..].iter().position(|c| c == ch)?;
        score += if pos == prev.wrapping_add(1) { 8 } else if is_sep(pos) { 6 } else { 1 };
        prev = pos;
        start = pos + 1;
    }
    Some(score)
}

/// Score text against a query; None means no match. Every word must match.
pub fn score(query: &str, text: &str) -> Option<i64> {
    let text: Vec<char> = text.to_lowercase().chars().collect();
    query.to_lowercase().split_whitespace().map(|t| token_score(&t.chars().collect::<Vec<_>>(), &text)).sum()
}

pub fn rank<T: Clone>(query: &str, items: &[T], key: impl Fn(&T) -> &str) -> Vec<T> {
    if query.trim().is_empty() {
        return items.to_vec();
    }
    let mut matched: Vec<(i64, &T)> = items.iter().filter_map(|it| Some((score(query, key(it))?, it))).collect();
    matched.sort_by_key(|(s, it)| (-s, key(it).len()));
    matched.into_iter().map(|(_, it)| it.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_name_matches_first() {
        let items = ["repos/claude-hub", "repos/other/hubble", "x/c-l-a-u-d-e"];
        assert_eq!(rank("hub", &items, |s| s)[0], "repos/claude-hub");
        assert_eq!(score("cld", "claude"), Some(15));
        assert!(score("zzz", "claude").is_none());
        assert!(score("claude hub", "repos/claude-hub").is_some());
        assert_eq!(rank("", &items, |s| s).len(), 3);
    }
}
