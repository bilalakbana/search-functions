//! Filtering as Bilal's fzf does it (`-i -e --no-sort`): case-insensitive substrings, every word of
//! the query must match, the input order is kept.

use crate::items::Item;

/// From this many candidates on, the filter runs on all cores.
const PARALLEL: usize = 50_000;

#[derive(Default)]
pub struct Filter {
    items: Vec<Item>,
    words: Vec<String>,
    /// Indices of the matching items, in input order; unused while there are no words.
    matches: Vec<usize>,
}

impl Filter {
    pub fn new() -> Filter {
        Filter::default()
    }

    /// Drops every item; the query stays.
    pub fn clear(&mut self) {
        self.items.clear();
        self.matches.clear();
    }

    pub fn push(&mut self, items: Vec<Item>) {
        let first = self.items.len();
        if first == 0 {
            self.items = items; // a whole list in one message: no copy
        } else {
            self.items.extend(items);
        }
        if !self.words.is_empty() {
            let new: Vec<usize> = (first..self.items.len()).collect();
            let new = self.matching(&new);
            self.matches.extend(new);
        }
    }

    pub fn set_query(&mut self, q: &str) {
        let words: Vec<String> = q.split_whitespace().map(str::to_lowercase).collect();
        if words == self.words {
            return;
        }
        // Every old word inside a new one: the new matches are a subset of the old ones.
        let narrower = !self.words.is_empty()
            && self
                .words
                .iter()
                .all(|o| words.iter().any(|n| n.contains(o.as_str())));
        let candidates = if narrower {
            std::mem::take(&mut self.matches)
        } else {
            (0..self.items.len()).collect()
        };
        self.words = words;
        self.matches = if self.words.is_empty() {
            Vec::new()
        } else {
            self.matching(&candidates)
        };
    }

    /// The candidates that match, in their order; large lists split over the cores.
    fn matching(&self, candidates: &[usize]) -> Vec<usize> {
        let filter = |part: &[usize]| {
            part.iter()
                .copied()
                .filter(|&i| self.is_match(i))
                .collect::<Vec<_>>()
        };
        if candidates.len() < PARALLEL {
            return filter(candidates);
        }
        let threads = std::thread::available_parallelism()
            .map_or(4, |n| n.get())
            .min(16);
        std::thread::scope(|scope| {
            let parts: Vec<_> = candidates
                .chunks(candidates.len().div_ceil(threads))
                .map(|part| scope.spawn(move || filter(part)))
                .collect();
            parts
                .into_iter()
                .flat_map(|t| t.join().expect("a filter thread panicked"))
                .collect()
        })
    }

    fn is_match(&self, i: usize) -> bool {
        let hay = self.items[i].haystack();
        self.words.iter().all(|w| hay.contains(w.as_str()))
    }

    /// The number of matching items.
    pub fn len(&self) -> usize {
        if self.words.is_empty() {
            self.items.len()
        } else {
            self.matches.len()
        }
    }

    /// The number of items.
    pub fn total(&self) -> usize {
        self.items.len()
    }

    /// The i-th matching item.
    pub fn get(&self, i: usize) -> Option<&Item> {
        if self.words.is_empty() {
            self.items.get(i)
        } else {
            self.items.get(*self.matches.get(i)?)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::ParseKind;
    use crate::items::parse;
    use std::time::{Duration, Instant};

    fn items(names: &[&str]) -> Vec<Item> {
        names
            .iter()
            .map(|n| parse(ParseKind::Files, n.as_bytes(), 0).unwrap())
            .collect()
    }

    fn shown(f: &Filter) -> Vec<String> {
        (0..f.len())
            .map(|i| f.get(i).unwrap().key.to_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn empty_query_shows_all() {
        let mut f = Filter::new();
        f.push(items(&["b", "a"]));
        assert_eq!(shown(&f), ["b", "a"]);
        assert_eq!(f.total(), 2);
        assert_eq!(f.get(2), None);
    }

    #[test]
    fn all_words_must_match() {
        let mut f = Filter::new();
        f.push(items(&["src/main.rs", "src/lib.rs", "docs/main.md"]));
        f.set_query("main src");
        assert_eq!(shown(&f), ["src/main.rs"]);
        f.set_query("  main   ");
        assert_eq!(shown(&f), ["src/main.rs", "docs/main.md"]);
    }

    #[test]
    fn case_insensitive() {
        let mut f = Filter::new();
        f.push(items(&["README.md", "Çay.txt", "other"]));
        f.set_query("readme");
        assert_eq!(shown(&f), ["README.md"]);
        f.set_query("ÇAY");
        assert_eq!(shown(&f), ["Çay.txt"]);
    }

    #[test]
    fn keeps_input_order() {
        let mut f = Filter::new();
        f.push(items(&["zz-x", "aa-x", "mm-x", "no"]));
        f.set_query("x");
        assert_eq!(shown(&f), ["zz-x", "aa-x", "mm-x"]);
    }

    #[test]
    fn large_lists_keep_input_order() {
        let names: Vec<String> = (0..(3 * PARALLEL))
            .map(|i| format!("{}-{i}", if i % 7 == 0 { "hit" } else { "miss" }))
            .collect();
        let mut f = Filter::new();
        f.push(items(&names.iter().map(String::as_str).collect::<Vec<_>>()));
        f.set_query("hit");
        let want: Vec<String> = (0..(3 * PARALLEL))
            .filter(|i| i % 7 == 0)
            .map(|i| format!("hit-{i}"))
            .collect();
        assert_eq!(shown(&f), want);
        f.push(items(&names.iter().map(String::as_str).collect::<Vec<_>>()));
        assert_eq!(f.len(), 2 * want.len());
        assert_eq!(f.get(want.len()).unwrap().key, "hit-0");
    }

    #[test]
    fn push_after_query_filters_new_items() {
        let mut f = Filter::new();
        f.set_query("rs");
        f.push(items(&["a.rs", "b.md"]));
        f.push(items(&["c.rs"]));
        assert_eq!(shown(&f), ["a.rs", "c.rs"]);
        assert_eq!(f.total(), 3);
    }

    #[test]
    fn narrowing_then_widening_query() {
        let mut f = Filter::new();
        f.push(items(&["abc", "abd", "xbc"]));
        f.set_query("ab");
        assert_eq!(shown(&f), ["abc", "abd"]);
        f.set_query("abc");
        assert_eq!(shown(&f), ["abc"]);
        f.set_query("bc");
        assert_eq!(shown(&f), ["abc", "xbc"]);
        f.set_query("");
        assert_eq!(shown(&f), ["abc", "abd", "xbc"]);
    }

    #[test]
    fn widening_query_refilters_all() {
        let mut f = Filter::new();
        f.push(items(&["abc", "xyz"]));
        f.set_query("abc");
        f.set_query("x");
        assert_eq!(shown(&f), ["xyz"]);
    }

    #[test]
    fn clear_keeps_query() {
        let mut f = Filter::new();
        f.set_query("a");
        f.push(items(&["a", "b"]));
        f.clear();
        assert_eq!(f.total(), 0);
        assert_eq!(f.len(), 0);
        f.push(items(&["ba", "c"]));
        assert_eq!(shown(&f), ["ba"]);
    }

    #[test]
    #[ignore = "speed: cargo test --release -- --ignored"]
    fn speed_1m_items() {
        // as many as fd lists in Bilal's home folder (971 657 with hidden and ignored files)
        let names: Vec<String> = (0..1_000_000)
            .map(|i| {
                format!(
                    "./Projects/app{}/src/module{}/File_{i}_name.rs",
                    i % 1000,
                    i % 37
                )
            })
            .collect();
        let mut f = Filter::new();
        f.push(
            names
                .iter()
                .map(|n| parse(ParseKind::Files, n.as_bytes(), 0).unwrap())
                .collect(),
        );
        for q in [
            "s",
            "sr",
            "src",
            "m",
            "ma",
            "mai",
            "file_1",
            "module3 file",
            "x",
            "",
        ] {
            let t = Instant::now();
            f.set_query(q);
            let took = t.elapsed();
            eprintln!("query {q:?}: {took:?}, {} matches", f.len());
            assert!(
                took < Duration::from_millis(30),
                "query {q:?} took {took:?}"
            );
        }
    }

    #[test]
    #[ignore = "speed: cargo test --release -- --ignored"]
    fn speed_300k_items() {
        let names: Vec<String> = (0..300_000)
            .map(|i| {
                format!(
                    "./project{}/src/module{}/file_{i}_name.rs",
                    i % 1000,
                    i % 37
                )
            })
            .collect();
        let mut f = Filter::new();
        f.push(
            names
                .iter()
                .map(|n| parse(ParseKind::Files, n.as_bytes(), 0).unwrap())
                .collect(),
        );
        let queries = [
            "f",
            "fi",
            "fil",
            "file_1",
            "file_12",
            "file_1",
            "mod",
            "module3 file",
            "x",
            "",
        ];
        for q in queries {
            let t = Instant::now();
            f.set_query(q);
            let took = t.elapsed();
            eprintln!("query {q:?}: {took:?}, {} matches", f.len());
            assert!(
                took < Duration::from_millis(30),
                "query {q:?} took {took:?}"
            );
        }
    }
}
