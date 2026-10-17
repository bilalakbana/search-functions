//! Filtering as Bilal's fzf does it (`-i -e --no-sort`): case-insensitive substrings, every word of
//! the query must match, with fzf's extended search (`!word`, `^start`, `end$`, `a | b`); the input
//! order is kept. File types turned off in Ctrl-S leave their items out too.

use std::collections::BTreeMap;

use crate::items::{Item, file_type};

/// How many items there are of each file type (`items::file_type`).
pub type TypeCounts = BTreeMap<Option<String>, usize>;

/// From this many candidates on, the filter runs on all cores.
const PARALLEL: usize = 50_000;

/// Where a term of the query is looked for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Anywhere,
    /// `^word`
    Start,
    /// `word$`
    End,
    /// `^word$`
    Whole,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Term {
    pub kind: Kind,
    /// `!word`: the items without it.
    pub left_out: bool,
    pub text: String,
}

impl Term {
    /// Whether `hay` holds the term's text where its kind says; as fzf, an anchor skips the spaces at
    /// that end unless the text has one there itself.
    fn found(&self, hay: &str) -> bool {
        let t = self.text.as_str();
        let start = |h: &'_ str| -> usize {
            if t.starts_with(char::is_whitespace) {
                0
            } else {
                h.len() - h.trim_start().len()
            }
        };
        let end = |h: &'_ str| -> usize {
            if t.ends_with(char::is_whitespace) {
                h.len()
            } else {
                h.trim_end().len()
            }
        };
        match self.kind {
            Kind::Anywhere => hay.contains(t),
            Kind::Start => hay[start(hay)..].starts_with(t),
            Kind::End => hay[..end(hay)].ends_with(t),
            Kind::Whole => {
                let (a, b) = (start(hay), end(hay));
                a <= b && &hay[a..b] == t
            }
        }
    }
}

/// What a filter's words mean, read as fzf reads them with `--exact`: every set of terms must match,
/// a set when any of its terms does (`a | b`), a left-out term (`!a`) when the item lacks it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    sets: Vec<Vec<Term>>,
}

impl Query {
    /// `typed` as fzf's `parseTerms` reads it (but `'`, which only switches fuzzy matching): words
    /// apart by spaces, `\ ` a space inside one; in lower case unless the case counts.
    pub fn parse(typed: &str, match_case: bool) -> Query {
        let typed = typed.replace("\\ ", "\t");
        let mut sets = Vec::new();
        let mut set: Vec<Term> = Vec::new();
        let (mut next_set, mut after_bar) = (false, false);
        for token in typed.split(' ').filter(|t| !t.is_empty()) {
            let token = token.replace('\t', " ");
            let token = if match_case {
                token
            } else {
                token.to_lowercase()
            };
            if !set.is_empty() && !after_bar && token == "|" {
                next_set = false;
                after_bar = true;
                continue;
            }
            after_bar = false;
            let mut text = token.as_str();
            let left_out = text.starts_with('!');
            if left_out {
                text = &text[1..];
            }
            let mut kind = Kind::Anywhere;
            if text != "$" && text.ends_with('$') {
                kind = Kind::End;
                text = &text[..text.len() - 1];
            }
            if let Some(rest) = text.strip_prefix('^') {
                kind = if kind == Kind::End {
                    Kind::Whole
                } else {
                    Kind::Start
                };
                text = rest;
            }
            if text.is_empty() {
                continue;
            }
            if next_set {
                sets.push(std::mem::take(&mut set));
            }
            set.push(Term {
                kind,
                left_out,
                text: text.to_string(),
            });
            next_set = true;
        }
        if !set.is_empty() {
            sets.push(set);
        }
        Query { sets }
    }

    pub fn is_empty(&self) -> bool {
        self.sets.is_empty()
    }

    pub fn matches(&self, hay: &str) -> bool {
        self.sets
            .iter()
            .all(|set| set.iter().any(|t| t.found(hay) != t.left_out))
    }

    /// The terms an item shows (the red marks): none left out.
    pub fn shown_terms(&self) -> impl Iterator<Item = &Term> {
        self.sets.iter().flatten().filter(|t| !t.left_out)
    }

    /// Only the sets without a left-out term: what an item has to match in lower case too.
    fn without_left_out(mut self) -> Query {
        self.sets.retain(|set| set.iter().all(|t| !t.left_out));
        self
    }

    /// The words when every set is one word looked for anywhere; else None.
    fn plain_words(&self) -> Option<Vec<&str>> {
        self.sets
            .iter()
            .map(|set| match set.as_slice() {
                [t] if t.kind == Kind::Anywhere && !t.left_out => Some(t.text.as_str()),
                _ => None,
            })
            .collect()
    }
}

#[derive(Default)]
pub struct Filter {
    items: Vec<Item>,
    /// The query as typed.
    typed: String,
    /// Ctrl-S, Match case: the case of the words counts.
    match_case: bool,
    /// What the items must match: in lower case, or as typed while the case counts.
    query: Query,
    /// What an item's lower-case text must match first: the query, or while the case counts its sets
    /// without a left-out word (a quick pass before the text is made).
    lower: Query,
    /// File types whose items are left out (`file_type`: the extension, None for none).
    hidden_types: Vec<Option<String>>,
    /// Indices of the matching items, in input order; unused while nothing filters (`active`).
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
        if self.active() {
            let new: Vec<usize> = (first..self.items.len()).collect();
            let new = self.matching(&new);
            self.matches.extend(new);
        }
    }

    pub fn set_query(&mut self, q: &str) {
        let query = Query::parse(q, self.match_case);
        let lower = if self.match_case {
            Query::parse(q, false).without_left_out()
        } else {
            query.clone()
        };
        self.typed = q.to_string();
        if query == self.query && lower == self.lower {
            return;
        }
        // Plain words, every old one inside a new one: the new matches are a subset of the old ones
        // (in lower case: with the case counting, an item may have the new word and not the old one).
        let narrower = !self.match_case
            && match (self.query.plain_words(), query.plain_words()) {
                (Some(old), Some(new)) => {
                    !old.is_empty() && old.iter().all(|o| new.iter().any(|n| n.contains(o)))
                }
                _ => false,
            };
        let candidates = if narrower {
            std::mem::take(&mut self.matches)
        } else {
            (0..self.items.len()).collect()
        };
        self.query = query;
        self.lower = lower;
        self.matches = if self.active() {
            self.matching(&candidates)
        } else {
            Vec::new()
        };
    }

    /// Whether the case of the words counts; the query is read again.
    pub fn set_match_case(&mut self, on: bool) {
        if on == self.match_case {
            return;
        }
        self.match_case = on;
        let typed = std::mem::take(&mut self.typed);
        self.query = Query::default(); // forces set_query to filter afresh
        self.lower = Query::default();
        self.set_query(&typed);
    }
    pub fn set_hidden_types(&mut self, types: Vec<Option<String>>) {
        if types == self.hidden_types {
            return;
        }
        self.hidden_types = types;
        let all: Vec<usize> = (0..self.items.len()).collect();
        self.matches = if self.active() {
            self.matching(&all)
        } else {
            Vec::new()
        };
    }

    /// How many items of each file type there are, the hidden ones too.
    pub fn types(&self) -> TypeCounts {
        let mut counts = BTreeMap::new();
        for item in &self.items {
            *counts.entry(file_type(&item.key)).or_insert(0) += 1;
        }
        counts
    }

    /// Whether anything filters: words, or file types left out.
    fn active(&self) -> bool {
        !self.query.is_empty() || !self.hidden_types.is_empty()
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
        let item = &self.items[i];
        if !self.hidden_types.is_empty() && self.hidden_types.contains(&file_type(&item.key)) {
            return false;
        }
        if !self.lower.matches(&item.haystack()) {
            return false;
        }
        // with the case counting, a second look at what the lower-case pass let through
        !self.match_case || self.query.matches(&item.text())
    }

    /// The number of matching items.
    pub fn len(&self) -> usize {
        if !self.active() {
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
        if !self.active() {
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

    /// What `query` lets through of `names`.
    fn through(names: &[&str], query: &str) -> Vec<String> {
        let mut f = Filter::new();
        f.push(items(names));
        f.set_query(query);
        shown(&f)
    }

    #[test]
    fn fzf_syntax_leaves_out_anchors_and_ors() {
        let names = [
            "src/app.rs",
            "src/tests.rs",
            "src/app.c",
            "docs/src.rs",
            "lib/core.go",
        ];
        assert_eq!(
            through(&names, "src !test .rs$"),
            ["src/app.rs", "docs/src.rs"]
        );
        assert_eq!(through(&names, "^src .rs$"), ["src/app.rs", "src/tests.rs"]);
        assert_eq!(
            through(&names, "^src/app.c$"),
            ["src/app.c"],
            "^…$: the whole"
        );
        assert_eq!(through(&names, "!^src !.rs$"), ["lib/core.go"]);
        assert_eq!(
            through(&names, "^src .c$ | go$"),
            ["src/app.c"],
            "| joins the terms on either side; the sets must all match"
        );
        assert_eq!(through(&names, ".c$ | .go$"), ["src/app.c", "lib/core.go"]);
    }

    #[test]
    fn fzf_syntax_edge_cases_as_fzf_reads_them() {
        let names = ["a b.txt", "a-b.txt", "x|y", "cost$", "!bang"];
        assert_eq!(
            through(&names, r"a\ b"),
            ["a b.txt"],
            "a backslash keeps the space"
        );
        assert_eq!(through(&names, "!"), names, "a lone ! is nothing");
        assert_eq!(through(&names, "^"), names, "a lone ^ is nothing");
        assert_eq!(through(&names, "$"), ["cost$"], "a lone $ is the character");
        assert_eq!(
            through(&names, "| y"),
            ["x|y"],
            "a | with nothing before is the character"
        );
        assert_eq!(
            through(&names, "!!"),
            ["a b.txt", "a-b.txt", "x|y", "cost$"]
        );
        assert_eq!(
            through(&["  indented"], "^indented"),
            ["  indented"],
            "as fzf, after leading spaces"
        );
    }

    #[test]
    fn a_left_out_word_follows_match_case() {
        let mut f = Filter::new();
        f.push(items(&["Foo.txt", "foo.txt", "bar"]));
        f.set_query("!foo");
        assert_eq!(shown(&f), ["bar"], "case aside");
        f.set_match_case(true);
        f.set_query("!Foo");
        assert_eq!(shown(&f), ["foo.txt", "bar"], "only Foo as typed");
    }

    #[test]
    fn a_query_that_widens_filters_every_item_again() {
        let mut f = Filter::new();
        f.push(items(&["abc", "xyz"]));
        f.set_query("a");
        assert_eq!(shown(&f), ["abc"]);
        f.set_query("!a");
        assert_eq!(shown(&f), ["xyz"], "not looked for among the rows with a");
        f.set_query("!ab");
        f.set_query("!abc");
        assert_eq!(shown(&f), ["xyz"]);
        f.set_query("!abcd");
        assert_eq!(shown(&f), ["abc", "xyz"]);
    }

    #[test]
    fn matching_lines_are_filtered_by_their_whole_row() {
        // Ctrl-Alt-F on Matching lines: the path and the line as shown (it let no row through)
        let rows: Vec<Item> = [
            &b"CODE_OF_CONDUCT.md\x007\x1fthe \x01interest\x02 of"[..],
            b"src/a.rs\x003\x1f\x01interest\x02 rate",
        ]
        .iter()
        .map(|r| parse(ParseKind::SearchLines, r, 0).unwrap())
        .collect();
        let mut f = Filter::new();
        f.push(rows);
        f.set_query("code");
        assert_eq!(shown(&f), ["CODE_OF_CONDUCT.md"], "the path");
        f.set_query("rate");
        assert_eq!(shown(&f), ["src/a.rs"], "the line");
        f.set_query(":3:");
        assert_eq!(
            shown(&f),
            ["src/a.rs"],
            "the row as shown, line number and all"
        );
        f.set_match_case(true);
        f.set_query("CODE");
        assert_eq!(shown(&f), ["CODE_OF_CONDUCT.md"]);
    }

    #[test]
    fn match_case_keeps_the_case_typed() {
        let mut f = Filter::new();
        f.push(items(&["README.md", "readme.txt", "ReadMe"]));
        f.set_match_case(true);
        f.set_query("README");
        assert_eq!(shown(&f), ["README.md"]);
        f.set_query("READMEx");
        assert_eq!(shown(&f), Vec::<String>::new());
        f.set_query("read");
        assert_eq!(
            shown(&f),
            ["readme.txt"],
            "not narrowed from the last words"
        );
        f.set_match_case(false);
        assert_eq!(shown(&f).len(), 3);
    }

    #[test]
    fn hidden_types_leave_their_files_out() {
        let mut f = Filter::new();
        f.push(items(&["a.rs", "b.dll", "Makefile", "c.RS", "x/.bashrc"]));
        f.set_hidden_types(vec![Some("dll".into()), None]);
        assert_eq!(shown(&f), ["a.rs", "c.RS"], "no extension is a type too");
        assert_eq!(f.total(), 5);
        f.set_query("c");
        assert_eq!(shown(&f), ["c.RS"], "with the words");
        f.set_query("");
        f.push(items(&["d.dll", "d.rs"]));
        assert_eq!(shown(&f), ["a.rs", "c.RS", "d.rs"], "new items too");
        f.set_hidden_types(Vec::new());
        assert_eq!(f.len(), 7);
    }

    #[test]
    fn types_count_every_item() {
        let mut f = Filter::new();
        f.push(items(&[
            "a.rs",
            "b.dll",
            "Makefile",
            "x/.bashrc",
            "c.rs",
            "d.tar.gz",
        ]));
        f.set_query("zzz");
        f.set_hidden_types(vec![Some("rs".into())]);
        let want: Vec<(Option<String>, usize)> = vec![
            (None, 2),
            (Some("dll".into()), 1),
            (Some("gz".into()), 1),
            (Some("rs".into()), 2),
        ];
        assert_eq!(f.types().into_iter().collect::<Vec<_>>(), want);
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
