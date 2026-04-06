use nucleo_matcher::{
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
    Config, Matcher, Utf32Str,
};

/// Fuzzy match a query against a list of items, returning (index, score) pairs sorted by score
pub fn fuzzy_match(query: &str, items: &[String]) -> Vec<(usize, u32)> {
    if query.is_empty() {
        return items.iter().enumerate().map(|(i, _)| (i, 0)).collect();
    }

    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::new(query, CaseMatching::Ignore, Normalization::Smart, AtomKind::Fuzzy);

    let mut results: Vec<(usize, u32)> = items
        .iter()
        .enumerate()
        .filter_map(|(idx, item)| {
            let mut buf = Vec::new();
            let haystack = Utf32Str::new(item, &mut buf);
            pattern.score(haystack, &mut matcher).map(|score| (idx, score))
        })
        .collect();

    // Sort by score descending
    results.sort_by(|a, b| b.1.cmp(&a.1));
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_query_returns_all() {
        let items = vec!["foo".to_string(), "bar".to_string(), "baz".to_string()];
        let results = fuzzy_match("", &items);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_fuzzy_match_basic() {
        let items = vec![
            "Internal Transfers".to_string(),
            "APO Click".to_string(),
            "Add Fixed Term".to_string(),
        ];
        let results = fuzzy_match("int", &items);
        assert!(!results.is_empty());
        // "Internal Transfers" should be the top match
        assert_eq!(results[0].0, 0);
    }

    #[test]
    fn test_no_match() {
        let items = vec!["foo".to_string(), "bar".to_string()];
        let results = fuzzy_match("zzz", &items);
        assert!(results.is_empty());
    }
}
