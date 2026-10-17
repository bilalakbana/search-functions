//! Ctrl-F: where a preview line holds what is looked for, by Find's options (Match case, Whole word,
//! Regular expression), as Notepad++'s Find does it. A regular expression is fancy-regex's: Perl's
//! syntax with look-around and back-references, the plain parts run by the regex crate's engines.

use std::ops::Range;
use std::time::{Duration, Instant};

use fancy_regex::{Regex, RegexBuilder};

use crate::model::FindOpts;

pub struct Finder {
    re: Regex,
    whole_word: bool,
}

impl Finder {
    /// None while nothing is typed; an error for a regular expression that does not parse.
    pub fn new(text: &str, opts: &FindOpts) -> Result<Option<Finder>, fancy_regex::Error> {
        if text.is_empty() {
            return Ok(None);
        }
        let pattern = if opts.regex {
            text.to_string()
        } else {
            fancy_regex::escape(text).into_owned()
        };
        let re = RegexBuilder::new(&pattern)
            .case_insensitive(!opts.match_case)
            .build()?;
        Ok(Some(Finder {
            re,
            whole_word: opts.whole_word,
        }))
    }

    /// Where it is in `line`: byte ranges in order, none empty, none overlapping; an error when a
    /// regular expression backtracks past fancy-regex's limit (it would not finish).
    pub fn matches(&self, line: &str) -> Result<Vec<Range<usize>>, fancy_regex::Error> {
        let mut found = Vec::new();
        let mut at = 0;
        while at <= line.len() {
            let Some(m) = self.re.find_from_pos(line, at)? else {
                break;
            };
            let r = m.range();
            if !r.is_empty() && (!self.whole_word || whole_word(line, r.clone())) {
                at = r.end;
                found.push(r);
            } else {
                // empty, or inside a word: look again from the next character, which may start one
                at = r.start + line[r.start..].chars().next().map_or(1, char::len_utf8);
            }
        }
        Ok(found)
    }
}

/// Why `find_all` found nothing to show.
#[derive(Debug, PartialEq)]
pub enum Trouble {
    /// A regular expression backtracked past fancy-regex's limit on a line.
    Bad,
    /// The time given was spent with lines left: one that backtracks under the limit on every line
    /// of a long preview would keep sf from answering for minutes.
    Slow,
}

/// Every match in `lines`, numbered from 0: (line, bytes). Stops with `Trouble` past the backtracking
/// limit, or once `budget` is spent while lines are left.
pub fn find_all(
    finder: &Finder,
    lines: impl Iterator<Item = String>,
    budget: Duration,
) -> Result<Vec<(usize, Range<usize>)>, Trouble> {
    let start = Instant::now();
    let mut hits = Vec::new();
    let mut lines = lines.enumerate().peekable();
    while let Some((i, line)) = lines.next() {
        let found = finder.matches(&line).map_err(|_| Trouble::Bad)?;
        hits.extend(found.into_iter().map(|r| (i, r)));
        if lines.peek().is_some() && start.elapsed() > budget {
            return Err(Trouble::Slow);
        }
    }
    Ok(hits)
}

/// No word character right before or after `r` in `line` (Notepad++'s Match whole word only).
fn whole_word(line: &str, r: Range<usize>) -> bool {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    !line[..r.start].chars().next_back().is_some_and(word)
        && !line[r.end..].chars().next().is_some_and(word)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn opts(match_case: bool, whole_word: bool, regex: bool) -> FindOpts {
        FindOpts {
            match_case,
            whole_word,
            regex,
        }
    }

    /// The parts of `line` found.
    fn found<'a>(text: &str, o: &FindOpts, line: &'a str) -> Vec<&'a str> {
        let f = Finder::new(text, o).unwrap().expect("a finder");
        f.matches(line)
            .unwrap()
            .into_iter()
            .map(|r| &line[r])
            .collect()
    }

    #[test]
    fn nothing_typed_finds_nothing() {
        assert!(Finder::new("", &FindOpts::default()).unwrap().is_none());
    }

    #[test]
    fn the_text_is_found_case_aside_with_its_byte_ranges() {
        let f = Finder::new("Config", &FindOpts::default())
            .unwrap()
            .unwrap();
        assert_eq!(
            f.matches("let config = load_CONFIG();").unwrap(),
            [4..10, 18..24]
        );
    }

    #[test]
    fn match_case_counts_the_case() {
        assert_eq!(
            found("config", &opts(true, false, false), "config Config CONFIG"),
            ["config"]
        );
    }

    #[test]
    fn without_regular_expression_the_text_is_literal() {
        assert_eq!(found("a.b(", &FindOpts::default(), "axb( a.b("), ["a.b("]);
    }

    #[test]
    fn whole_word_leaves_out_matches_inside_words() {
        let line = "config configs reconfig config_x (config) config";
        assert_eq!(
            found("config", &opts(false, true, false), line),
            ["config", "config", "config"]
        );
        let f = Finder::new("config", &opts(false, true, false))
            .unwrap()
            .unwrap();
        assert_eq!(f.matches(line).unwrap(), [0..6, 34..40, 42..48]);
    }

    #[test]
    fn whole_word_finds_one_starting_inside_a_match_it_left_out() {
        // "a a" at 1 has a 'b' before it; the one at 3 is a whole word
        let f = Finder::new("a a", &opts(false, true, false))
            .unwrap()
            .unwrap();
        assert_eq!(f.matches("ba a a").unwrap(), [Range { start: 3, end: 6 }]);
    }

    #[test]
    fn whole_word_means_no_word_character_on_either_side() {
        // as Notepad++: bracketed by non-word characters, also when the text ends in one
        assert_eq!(found("(x)", &opts(false, true, false), "f(x) (x)"), ["(x)"]);
        assert_eq!(
            found("é", &opts(false, true, false), "café é"),
            ["é"],
            "letters beyond ASCII are word characters"
        );
    }

    #[test]
    fn a_regular_expression_is_found_case_aside() {
        assert_eq!(
            found("lo+ad", &opts(false, false, true), "load LOOAD lad"),
            ["load", "LOOAD"]
        );
        assert_eq!(
            found("lo+ad", &opts(true, false, true), "load LOOAD lad"),
            ["load"]
        );
    }

    #[test]
    fn whole_word_works_with_a_regular_expression_too() {
        assert_eq!(
            found(r"con\w*", &opts(false, true, true), "config reconfig con"),
            ["config", "con"]
        );
    }

    #[test]
    fn a_regular_expression_that_does_not_parse_is_an_error() {
        assert!(Finder::new("a(", &opts(false, false, true)).is_err());
        assert!(
            Finder::new("a(", &FindOpts::default()).is_ok(),
            "literal text never fails"
        );
    }

    #[test]
    fn empty_matches_are_not_found() {
        assert_eq!(found("x*", &opts(false, false, true), "axxb"), ["xx"]);
        assert!(found("^", &opts(false, false, true), "abc").is_empty());
        assert!(found(r"\b", &opts(false, true, true), "a b").is_empty());
    }

    #[test]
    fn perl_syntax_works_in_a_regular_expression() {
        let re = opts(false, false, true);
        assert_eq!(
            found("foo(?=bar)", &re, "foobaz foobar"),
            ["foo"],
            "look-ahead"
        );
        assert_eq!(
            found(r"(?<=\$)\d+", &re, "$15 and 20"),
            ["15"],
            "look-behind"
        );
        assert_eq!(
            found(r"(\w)\1", &re, "hello aab"),
            ["ll", "aa"],
            "a back-reference"
        );
        assert_eq!(found(r"(?<!\d)\d{2}\b", &re, "123 45"), ["45"]);
    }

    #[test]
    fn a_regular_expression_too_hard_to_finish_is_an_error() {
        let f = Finder::new(r"(a|aa)+\1b", &opts(false, false, true))
            .unwrap()
            .unwrap();
        assert!(
            f.matches(&"a".repeat(40)).is_err(),
            "stops at the backtracking limit"
        );
    }

    #[test]
    fn find_all_numbers_the_lines_and_stops_when_its_time_is_spent() {
        let f = Finder::new("x", &FindOpts::default()).unwrap().unwrap();
        let lines = || ["x a x", "b", "X"].map(String::from).into_iter();
        let all = find_all(&f, lines(), Duration::from_secs(10));
        assert_eq!(all, Ok(vec![(0, 0..1), (0, 4..5), (2, 0..1)]));
        assert_eq!(find_all(&f, lines(), Duration::ZERO), Err(Trouble::Slow));
        let one = find_all(&f, ["x".to_string()].into_iter(), Duration::ZERO);
        assert_eq!(one, Ok(vec![(0, 0..1)]), "nothing left when the time is up");
        let hard = Finder::new(r"(a|aa)+\1b", &opts(false, false, true))
            .unwrap()
            .unwrap();
        let lines = ["ab".to_string(), "a".repeat(40)].into_iter();
        assert_eq!(
            find_all(&hard, lines, Duration::from_secs(10)),
            Err(Trouble::Bad)
        );
    }

    #[test]
    fn letters_beyond_ascii_are_found_case_aside() {
        assert_eq!(found("é", &FindOpts::default(), "café É"), ["é", "É"]);
    }
}
