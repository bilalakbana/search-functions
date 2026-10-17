//! Ctrl-F: where a preview line holds what is looked for, by Find's options (Match case, Whole word,
//! Regular expression), as Notepad++'s Find does it.

use std::ops::Range;

use regex::{Regex, RegexBuilder};

use crate::model::FindOpts;

pub struct Finder {
    re: Regex,
    whole_word: bool,
}

impl Finder {
    /// None while nothing is typed; an error for a regular expression that does not parse.
    pub fn new(text: &str, opts: &FindOpts) -> Result<Option<Finder>, regex::Error> {
        if text.is_empty() {
            return Ok(None);
        }
        let pattern = if opts.regex {
            text.to_string()
        } else {
            regex::escape(text)
        };
        let re = RegexBuilder::new(&pattern)
            .case_insensitive(!opts.match_case)
            .build()?;
        Ok(Some(Finder {
            re,
            whole_word: opts.whole_word,
        }))
    }

    /// Where it is in `line`: byte ranges in order, none empty, none overlapping.
    pub fn matches(&self, line: &str) -> Vec<Range<usize>> {
        let mut found = Vec::new();
        let mut at = 0;
        while at <= line.len() {
            let Some(m) = self.re.find_at(line, at) else {
                break;
            };
            if !m.is_empty() && (!self.whole_word || whole_word(line, m.range())) {
                found.push(m.range());
                at = m.end();
            } else {
                // empty, or inside a word: look again from the next character, which may start one
                at = m.start() + line[m.start()..].chars().next().map_or(1, char::len_utf8);
            }
        }
        found
    }
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
        f.matches(line).into_iter().map(|r| &line[r]).collect()
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
        assert_eq!(f.matches("let config = load_CONFIG();"), [4..10, 18..24]);
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
        assert_eq!(f.matches(line), [0..6, 34..40, 42..48]);
    }

    #[test]
    fn whole_word_finds_one_starting_inside_a_match_it_left_out() {
        // "a a" at 1 has a 'b' before it; the one at 3 is a whole word
        let f = Finder::new("a a", &opts(false, true, false))
            .unwrap()
            .unwrap();
        assert_eq!(f.matches("ba a a"), [Range { start: 3, end: 6 }]);
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
    fn letters_beyond_ascii_are_found_case_aside() {
        assert_eq!(found("é", &FindOpts::default(), "café É"), ["é", "É"]);
    }
}
