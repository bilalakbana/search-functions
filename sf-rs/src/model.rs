//! The tabs, their options and the defaults the spec sets for them.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Files,
    Content,
    /// Content's search through everything: binaries, archives, with Documents PDF and Office files
    /// (advancedsearch, advancedsearch_plus, detailedsearch).
    Deep,
    Folders,
    Git,
    /// The manual: what the tabs, keys, options and commands do, with examples.
    Help,
}

impl Tab {
    pub const ALL: [Tab; 6] = [
        Tab::Files,
        Tab::Content,
        Tab::Deep,
        Tab::Folders,
        Tab::Git,
        Tab::Help,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Files => "Files",
            Tab::Content => "Content",
            Tab::Deep => "Deep",
            Tab::Folders => "Folders",
            Tab::Git => "Git",
            Tab::Help => "Help",
        }
    }

    /// The tabs whose query is a ugrep pattern.
    pub fn searches(self) -> bool {
        matches!(self, Tab::Content | Tab::Deep)
    }

    fn index(self) -> usize {
        Tab::ALL.iter().position(|&t| t == self).unwrap()
    }

    pub fn next(self) -> Tab {
        Tab::ALL[(self.index() + 1) % Tab::ALL.len()]
    }

    pub fn prev(self) -> Tab {
        Tab::ALL[(self.index() + Tab::ALL.len() - 1) % Tab::ALL.len()]
    }

    /// "files", "Content", ... (the command-line argument; "search" is still taken).
    pub fn from_name(name: &str) -> Option<Tab> {
        if name.eq_ignore_ascii_case("search") {
            return Some(Tab::Content); // the name it had first
        }
        Tab::ALL
            .into_iter()
            .find(|t| t.title().eq_ignore_ascii_case(name))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opt {
    Hidden,
    Ignored,
    MatchingLines,
    /// Content: every match with the text around it (detailedsearch).
    EachMatch,
    Archives,
    /// Deep: PDF, Office and e-book files as text (ugrep+).
    Documents,
    AllBranches,
    /// Content, Deep, a file: every word on one line (ugrep's Boolean query, `--bool`).
    WordsOnALine,
    /// Content, Deep, a file: every word in the file, on any lines (`--bool --files`).
    WordsInTheFile,
    /// The case of what is typed counts (else smart case for a pattern, none for a filter).
    MatchCase,
    /// Git: a commit and a file's history (Ctrl-U) show the changed files whole.
    WholeFiles,
    /// Ctrl-F: not inside a longer word.
    WholeWord,
    /// Ctrl-F: what is typed is a regular expression.
    Regex,
    /// Searching versions (Git, Ctrl-Shift-Alt-F): binary files are searched as text too.
    Binary,
    /// A pattern with an upper-case letter counts the case (ugrep's smart case); off with Match case
    /// off: case aside.
    SmartCase,
}

impl Opt {
    /// The checkbox label; Folders says "folders" where the others say "files".
    pub fn label(self, tab: Tab) -> &'static str {
        let folders = tab == Tab::Folders;
        match self {
            Opt::Hidden if folders => "Hidden folders",
            Opt::Hidden => "Hidden files",
            Opt::Ignored if folders => "Ignored folders",
            Opt::Ignored => "Ignored files",
            Opt::MatchingLines => "Matching lines",
            Opt::EachMatch => "Each match",
            Opt::Archives => "Archives & documents",
            Opt::Documents => "Documents",
            Opt::AllBranches => "All branches",
            Opt::WordsOnALine => "All words on a line",
            Opt::WordsInTheFile => "All words in the file",
            Opt::MatchCase => "Match case",
            Opt::WholeFiles => "Whole files",
            Opt::WholeWord => "Whole word",
            Opt::Regex => "Regular expression",
            Opt::Binary => "Binary files",
            Opt::SmartCase => "Smart case",
        }
    }

    /// The dim explanation next to the label.
    pub fn hint(self) -> &'static str {
        match self {
            Opt::Hidden => "names starting with a dot",
            Opt::Ignored => ".gitignore'd",
            Opt::MatchingLines => "list lines, not files",
            Opt::EachMatch => "every match, the text around it",
            Opt::Archives => "zip, tar, 7z, PDF, Office…",
            Opt::Documents => "PDF, Office, e-books as text (slower)",
            Opt::AllBranches => "not only the current one",
            Opt::WordsOnALine => "a b: both on one line",
            Opt::WordsInTheFile => "a b: both in the file, any lines",
            Opt::MatchCase => "A is not a",
            Opt::WholeFiles => "the changed files whole, not only around the changes",
            Opt::WholeWord => "not inside a longer word",
            Opt::Regex => r"a.*b  \d+  (x|y)",
            Opt::Binary => "searched as text",
            Opt::SmartCase => "an upper-case letter counts the case",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FilesOpts {
    pub hidden: bool,
    pub ignored: bool,
    pub match_case: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ContentOpts {
    pub matching_lines: bool,
    /// Each match with ~50 characters around it; never on together with `matching_lines`.
    pub each_match: bool,
    pub archives: bool,
    pub hidden: bool,
    pub ignored: bool,
    /// The words of the pattern all on one line; never on together with `words_in_the_file`.
    pub words_on_a_line: bool,
    pub words_in_the_file: bool,
    /// The pattern not inside a longer word (`-w`).
    pub whole_word: bool,
    /// What is typed is a regular expression; off: the text as typed.
    pub regex: bool,
    /// An upper-case letter counts the case; never on together with `match_case`.
    pub smart_case: bool,
    pub match_case: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeepOpts {
    pub each_match: bool,
    pub documents: bool,
    pub hidden: bool,
    pub ignored: bool,
    pub words_on_a_line: bool,
    pub words_in_the_file: bool,
    pub whole_word: bool,
    pub regex: bool,
    pub smart_case: bool,
    pub match_case: bool,
}

/// Ctrl-T, the lines of one file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InFileOpts {
    pub words_on_a_line: bool,
    pub words_in_the_file: bool,
    pub smart_case: bool,
    pub match_case: bool,
}

/// Git, Ctrl-Shift-Alt-F: searching the files of every version (or those chosen) of the repository.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VersionsOpts {
    pub whole_word: bool,
    /// Perl's (`git grep -P`); off: the text as typed (`-F`).
    pub regex: bool,
    pub binary: bool,
    /// PDF, Office, e-books, archives: each version's file through ugrep(+), no copy on disk.
    pub documents: bool,
    pub smart_case: bool,
    pub match_case: bool,
}

impl Default for VersionsOpts {
    fn default() -> Self {
        VersionsOpts {
            whole_word: false,
            regex: true,
            binary: false,
            documents: false,
            smart_case: false,
            match_case: false,
        }
    }
}

/// Ctrl-F, finding in the preview (as Notepad++'s Find); all off: the text as typed, case aside.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FindOpts {
    pub match_case: bool,
    pub whole_word: bool,
    pub regex: bool,
    pub smart_case: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FoldersOpts {
    pub hidden: bool,
    pub ignored: bool,
    pub match_case: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GitOpts {
    pub all_branches: bool,
    pub whole_files: bool,
    pub match_case: bool,
}

impl Default for FilesOpts {
    fn default() -> Self {
        FilesOpts {
            hidden: false,
            ignored: true,
            match_case: false,
        }
    }
}

impl Default for ContentOpts {
    fn default() -> Self {
        ContentOpts {
            matching_lines: false,
            each_match: false,
            archives: false,
            hidden: false,
            ignored: true,
            words_on_a_line: false,
            words_in_the_file: false,
            whole_word: false,
            regex: true,
            smart_case: false,
            match_case: false,
        }
    }
}

impl Default for DeepOpts {
    fn default() -> Self {
        DeepOpts {
            each_match: false,
            documents: false,
            hidden: false,
            ignored: true,
            words_on_a_line: false,
            words_in_the_file: false,
            whole_word: false,
            regex: true,
            smart_case: false,
            match_case: false,
        }
    }
}

impl Default for InFileOpts {
    fn default() -> Self {
        // as the fzf window did: its fzf kept the lines with every word typed (Bilal)
        InFileOpts {
            words_on_a_line: true,
            words_in_the_file: false,
            smart_case: false,
            match_case: false,
        }
    }
}

impl Default for FoldersOpts {
    fn default() -> Self {
        FoldersOpts {
            hidden: false,
            ignored: true,
            match_case: false,
        }
    }
}

impl Default for GitOpts {
    fn default() -> Self {
        GitOpts {
            all_branches: true,
            whole_files: false,
            match_case: false,
        }
    }
}

/// Every tab's options, as saved in the config file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub files: FilesOpts,
    /// `[search]` in files from when the tab was called Search
    #[serde(alias = "search")]
    pub content: ContentOpts,
    pub deep: DeepOpts,
    pub in_file: InFileOpts,
    pub find: FindOpts,
    pub versions: VersionsOpts,
    pub folders: FoldersOpts,
    pub git: GitOpts,
}

impl Options {
    /// The tab's options in popup order, with their values.
    pub fn list(&self, tab: Tab) -> Vec<(Opt, bool)> {
        let order: &[Opt] = match tab {
            Tab::Files | Tab::Folders => &[Opt::Hidden, Opt::Ignored, Opt::MatchCase],
            Tab::Content => &[
                Opt::MatchingLines,
                Opt::EachMatch,
                Opt::Archives,
                Opt::Hidden,
                Opt::Ignored,
                Opt::WordsOnALine,
                Opt::WordsInTheFile,
                Opt::WholeWord,
                Opt::Regex,
                Opt::SmartCase,
                Opt::MatchCase,
            ],
            Tab::Deep => &[
                Opt::EachMatch,
                Opt::Documents,
                Opt::Hidden,
                Opt::Ignored,
                Opt::WordsOnALine,
                Opt::WordsInTheFile,
                Opt::WholeWord,
                Opt::Regex,
                Opt::SmartCase,
                Opt::MatchCase,
            ],
            Tab::Git => &[Opt::AllBranches, Opt::WholeFiles, Opt::MatchCase],
            Tab::Help => &[],
        };
        order.iter().map(|&o| (o, self.get(tab, o))).collect()
    }

    /// An option the tab does not have is off.
    pub fn get(&self, tab: Tab, opt: Opt) -> bool {
        self.clone().slot(tab, opt).is_some_and(|v| *v)
    }

    /// Toggling an option the tab does not have does nothing. Matching lines and Each match are two
    /// ways to list the matches: turning one on turns the other off.
    pub fn toggle(&mut self, tab: Tab, opt: Opt) {
        let Some(v) = self.slot(tab, opt) else {
            return;
        };
        *v = !*v;
        if !*v {
            return;
        }
        // smart case and match case: one or the other
        let other = match opt {
            Opt::SmartCase => Some(Opt::MatchCase),
            Opt::MatchCase => Some(Opt::SmartCase),
            _ => None,
        };
        if let Some(other) = other.and_then(|o| self.slot(tab, o)) {
            *other = false;
        }
        match (tab, opt) {
            (Tab::Content, Opt::MatchingLines) => self.content.each_match = false,
            (Tab::Content, Opt::EachMatch) => self.content.matching_lines = false,
            (Tab::Content, Opt::WordsOnALine) => self.content.words_in_the_file = false,
            (Tab::Content, Opt::WordsInTheFile) => self.content.words_on_a_line = false,
            (Tab::Deep, Opt::WordsOnALine) => self.deep.words_in_the_file = false,
            (Tab::Deep, Opt::WordsInTheFile) => self.deep.words_on_a_line = false,
            _ => {}
        }
    }

    /// The options of a search in one file (Ctrl-T), in popup order.
    pub fn in_file_list(&self) -> Vec<(Opt, bool)> {
        let f = &self.in_file;
        vec![
            (Opt::WordsOnALine, f.words_on_a_line),
            (Opt::WordsInTheFile, f.words_in_the_file),
            (Opt::SmartCase, f.smart_case),
            (Opt::MatchCase, f.match_case),
        ]
    }

    /// Toggles an option of the search in one file; the two kinds of words turn each other off.
    pub fn toggle_in_file(&mut self, opt: Opt) {
        let f = &mut self.in_file;
        match opt {
            Opt::WordsOnALine => {
                f.words_on_a_line = !f.words_on_a_line;
                f.words_in_the_file &= !f.words_on_a_line;
            }
            Opt::WordsInTheFile => {
                f.words_in_the_file = !f.words_in_the_file;
                f.words_on_a_line &= !f.words_in_the_file;
            }
            Opt::SmartCase => {
                f.smart_case = !f.smart_case;
                f.match_case &= !f.smart_case;
            }
            Opt::MatchCase => {
                f.match_case = !f.match_case;
                f.smart_case &= !f.match_case;
            }
            _ => {}
        }
    }

    /// Ctrl-F's options, in popup order.
    pub fn find_list(&self) -> Vec<(Opt, bool)> {
        let f = &self.find;
        vec![
            (Opt::WholeWord, f.whole_word),
            (Opt::Regex, f.regex),
            (Opt::SmartCase, f.smart_case),
            (Opt::MatchCase, f.match_case),
        ]
    }

    /// The options of searching versions (Git, Ctrl-Shift-Alt-F), in popup order.
    pub fn versions_list(&self) -> Vec<(Opt, bool)> {
        let v = &self.versions;
        vec![
            (Opt::WholeWord, v.whole_word),
            (Opt::Regex, v.regex),
            (Opt::Binary, v.binary),
            (Opt::Documents, v.documents),
            (Opt::SmartCase, v.smart_case),
            (Opt::MatchCase, v.match_case),
        ]
    }

    /// Toggles one of the options of searching versions; another does nothing.
    pub fn toggle_versions(&mut self, opt: Opt) {
        let v = &mut self.versions;
        let slot = match opt {
            Opt::WholeWord => &mut v.whole_word,
            Opt::Regex => &mut v.regex,
            Opt::Binary => &mut v.binary,
            Opt::Documents => &mut v.documents,
            Opt::SmartCase => &mut v.smart_case,
            Opt::MatchCase => &mut v.match_case,
            _ => return,
        };
        *slot = !*slot;
        match opt {
            Opt::SmartCase => v.match_case &= !v.smart_case,
            Opt::MatchCase => v.smart_case &= !v.match_case,
            _ => {}
        }
    }

    /// Toggles one of Ctrl-F's options; another does nothing.
    pub fn toggle_find(&mut self, opt: Opt) {
        let f = &mut self.find;
        match opt {
            Opt::MatchCase => {
                f.match_case = !f.match_case;
                f.smart_case &= !f.match_case;
            }
            Opt::SmartCase => {
                f.smart_case = !f.smart_case;
                f.match_case &= !f.smart_case;
            }
            Opt::WholeWord => f.whole_word = !f.whole_word,
            Opt::Regex => f.regex = !f.regex,
            _ => {}
        }
    }

    fn slot(&mut self, tab: Tab, opt: Opt) -> Option<&mut bool> {
        match (tab, opt) {
            (Tab::Files, Opt::Hidden) => Some(&mut self.files.hidden),
            (Tab::Files, Opt::Ignored) => Some(&mut self.files.ignored),
            (Tab::Files, Opt::MatchCase) => Some(&mut self.files.match_case),
            (Tab::Content, Opt::WordsOnALine) => Some(&mut self.content.words_on_a_line),
            (Tab::Content, Opt::WordsInTheFile) => Some(&mut self.content.words_in_the_file),
            (Tab::Content, Opt::WholeWord) => Some(&mut self.content.whole_word),
            (Tab::Content, Opt::Regex) => Some(&mut self.content.regex),
            (Tab::Content, Opt::SmartCase) => Some(&mut self.content.smart_case),
            (Tab::Content, Opt::MatchCase) => Some(&mut self.content.match_case),
            (Tab::Deep, Opt::WordsOnALine) => Some(&mut self.deep.words_on_a_line),
            (Tab::Deep, Opt::WordsInTheFile) => Some(&mut self.deep.words_in_the_file),
            (Tab::Deep, Opt::WholeWord) => Some(&mut self.deep.whole_word),
            (Tab::Deep, Opt::Regex) => Some(&mut self.deep.regex),
            (Tab::Deep, Opt::SmartCase) => Some(&mut self.deep.smart_case),
            (Tab::Deep, Opt::MatchCase) => Some(&mut self.deep.match_case),
            (Tab::Folders, Opt::MatchCase) => Some(&mut self.folders.match_case),
            (Tab::Git, Opt::MatchCase) => Some(&mut self.git.match_case),
            (Tab::Git, Opt::WholeFiles) => Some(&mut self.git.whole_files),
            (Tab::Content, Opt::MatchingLines) => Some(&mut self.content.matching_lines),
            (Tab::Content, Opt::EachMatch) => Some(&mut self.content.each_match),
            (Tab::Content, Opt::Archives) => Some(&mut self.content.archives),
            (Tab::Content, Opt::Hidden) => Some(&mut self.content.hidden),
            (Tab::Content, Opt::Ignored) => Some(&mut self.content.ignored),
            (Tab::Deep, Opt::EachMatch) => Some(&mut self.deep.each_match),
            (Tab::Deep, Opt::Documents) => Some(&mut self.deep.documents),
            (Tab::Deep, Opt::Hidden) => Some(&mut self.deep.hidden),
            (Tab::Deep, Opt::Ignored) => Some(&mut self.deep.ignored),
            (Tab::Folders, Opt::Hidden) => Some(&mut self.folders.hidden),
            (Tab::Folders, Opt::Ignored) => Some(&mut self.folders.ignored),
            (Tab::Git, Opt::AllBranches) => Some(&mut self.git.all_branches),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_cycle_wraps() {
        assert_eq!(Tab::Git.next(), Tab::Help);
        assert_eq!(Tab::Help.next(), Tab::Files);
        assert_eq!(Tab::Files.prev(), Tab::Help);
        assert_eq!(Tab::Files.next(), Tab::Content);
        assert_eq!(Tab::Content.prev(), Tab::Files);
    }

    #[test]
    fn from_name_ignores_case() {
        assert_eq!(Tab::from_name("content"), Some(Tab::Content));
        assert_eq!(
            Tab::from_name("search"),
            Some(Tab::Content),
            "the name it had first"
        );
        assert_eq!(Tab::Content.title(), "Content");
        assert_eq!(Tab::from_name("Git"), Some(Tab::Git));
        assert_eq!(Tab::from_name("lines"), None);
    }

    #[test]
    fn search_popup_order() {
        let opts = Options::default();
        let order: Vec<Opt> = opts
            .list(Tab::Content)
            .into_iter()
            .map(|(o, _)| o)
            .collect();
        assert_eq!(
            order,
            [
                Opt::MatchingLines,
                Opt::EachMatch,
                Opt::Archives,
                Opt::Hidden,
                Opt::Ignored,
                Opt::WordsOnALine,
                Opt::WordsInTheFile,
                Opt::WholeWord,
                Opt::Regex,
                Opt::SmartCase,
                Opt::MatchCase
            ]
        );
    }

    #[test]
    fn each_match_and_matching_lines_exclude_each_other() {
        let mut o = Options::default();
        o.toggle(Tab::Content, Opt::MatchingLines);
        o.toggle(Tab::Content, Opt::EachMatch);
        assert!(o.content.each_match && !o.content.matching_lines);
        o.toggle(Tab::Content, Opt::MatchingLines);
        assert!(o.content.matching_lines && !o.content.each_match);
        o.toggle(Tab::Content, Opt::MatchingLines);
        assert!(
            !o.content.matching_lines && !o.content.each_match,
            "off again: files"
        );
        let old: Options = toml::from_str("[content]\nmatching_lines = true\n").unwrap();
        assert!(!old.content.each_match, "a config from before has it off");
    }

    #[test]
    fn deep_sits_between_content_and_folders() {
        assert_eq!(Tab::Content.next(), Tab::Deep);
        assert_eq!(Tab::Deep.next(), Tab::Folders);
        assert_eq!(Tab::Folders.prev(), Tab::Deep);
        assert_eq!(Tab::from_name("deep"), Some(Tab::Deep));
        assert_eq!(Tab::Deep.title(), "Deep");
        assert!(Tab::Content.searches() && Tab::Deep.searches());
        assert!(!Tab::Files.searches() && !Tab::Folders.searches() && !Tab::Git.searches());
    }

    #[test]
    fn help_is_a_tab_without_options() {
        assert_eq!(Tab::from_name("help"), Some(Tab::Help));
        assert_eq!(Tab::Help.title(), "Help");
        assert!(!Tab::Help.searches());
        assert!(Options::default().list(Tab::Help).is_empty());
    }

    #[test]
    fn deep_options_and_defaults() {
        assert_eq!(
            Options::default().list(Tab::Deep),
            [
                (Opt::EachMatch, false),
                (Opt::Documents, false),
                (Opt::Hidden, false),
                (Opt::Ignored, true),
                (Opt::WordsOnALine, false),
                (Opt::WordsInTheFile, false),
                (Opt::WholeWord, false),
                (Opt::Regex, true),
                (Opt::SmartCase, false),
                (Opt::MatchCase, false)
            ]
        );
        assert_eq!(Opt::Documents.label(Tab::Deep), "Documents");
        let old: Options = toml::from_str("[content]\nmatching_lines = true\n").unwrap();
        assert_eq!(
            old.deep,
            DeepOpts::default(),
            "a config from before has Deep's defaults"
        );
    }

    #[test]
    fn deep_each_match_leaves_content_alone() {
        let mut o = Options::default();
        o.toggle(Tab::Content, Opt::MatchingLines);
        o.toggle(Tab::Deep, Opt::EachMatch);
        assert!(o.deep.each_match);
        assert!(o.content.matching_lines && !o.content.each_match);
        o.toggle(Tab::Deep, Opt::Documents);
        assert!(o.deep.documents && !o.content.archives);
    }

    #[test]
    fn smart_case_is_an_option_of_every_pattern_off_and_never_with_match_case() {
        let mut o = Options::default();
        let has = |list: Vec<(Opt, bool)>| list.contains(&(Opt::SmartCase, false));
        assert!(
            has(o.list(Tab::Content)) && has(o.list(Tab::Deep)),
            "off by default"
        );
        assert!(has(o.in_file_list()) && has(o.find_list()) && has(o.versions_list()));
        for tab in [Tab::Files, Tab::Folders, Tab::Git] {
            assert!(
                !o.list(tab).iter().any(|(opt, _)| *opt == Opt::SmartCase),
                "{tab:?}: a filter"
            );
        }
        let at = |list: Vec<(Opt, bool)>, opt| list.iter().position(|(x, _)| *x == opt).unwrap();
        assert_eq!(
            at(o.list(Tab::Content), Opt::SmartCase) + 1,
            at(o.list(Tab::Content), Opt::MatchCase),
            "right above Match case"
        );
        o.toggle(Tab::Content, Opt::MatchCase);
        o.toggle(Tab::Content, Opt::SmartCase);
        assert!(
            o.content.smart_case && !o.content.match_case,
            "one or the other"
        );
        o.toggle(Tab::Content, Opt::MatchCase);
        assert!(!o.content.smart_case && o.content.match_case);
        assert!(!o.deep.smart_case, "Deep's own");
        o.toggle_in_file(Opt::SmartCase);
        o.toggle_in_file(Opt::MatchCase);
        assert!(o.in_file.match_case && !o.in_file.smart_case);
        o.toggle_find(Opt::MatchCase);
        o.toggle_find(Opt::SmartCase);
        assert!(o.find.smart_case && !o.find.match_case);
        o.toggle_versions(Opt::SmartCase);
        assert!(o.versions.smart_case);
        o.toggle_versions(Opt::MatchCase);
        assert!(o.versions.match_case && !o.versions.smart_case);
        let old: Options = toml::from_str("[content]\nmatch_case = false\n").unwrap();
        assert!(!old.content.smart_case, "a config from before: case aside");
        assert_eq!(Opt::SmartCase.label(Tab::Content), "Smart case");
    }

    #[test]
    fn every_tab_can_match_case_and_the_searches_take_words() {
        let o = Options::default();
        let opts = |tab| {
            o.list(tab)
                .into_iter()
                .map(|(opt, _)| opt)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            opts(Tab::Files),
            [Opt::Hidden, Opt::Ignored, Opt::MatchCase]
        );
        assert_eq!(
            opts(Tab::Content),
            [
                Opt::MatchingLines,
                Opt::EachMatch,
                Opt::Archives,
                Opt::Hidden,
                Opt::Ignored,
                Opt::WordsOnALine,
                Opt::WordsInTheFile,
                Opt::WholeWord,
                Opt::Regex,
                Opt::SmartCase,
                Opt::MatchCase
            ]
        );
        assert_eq!(
            opts(Tab::Deep),
            [
                Opt::EachMatch,
                Opt::Documents,
                Opt::Hidden,
                Opt::Ignored,
                Opt::WordsOnALine,
                Opt::WordsInTheFile,
                Opt::WholeWord,
                Opt::Regex,
                Opt::SmartCase,
                Opt::MatchCase
            ]
        );
        assert_eq!(
            opts(Tab::Folders),
            [Opt::Hidden, Opt::Ignored, Opt::MatchCase]
        );
        assert_eq!(
            opts(Tab::Git),
            [Opt::AllBranches, Opt::WholeFiles, Opt::MatchCase]
        );
        assert!(!o.get(Tab::Git, Opt::WholeFiles), "off: as git shows it");
        for tab in Tab::ALL {
            assert!(!o.get(tab, Opt::MatchCase), "{tab:?}: off, as before");
        }
        assert!(!o.get(Tab::Content, Opt::WordsOnALine) && !o.get(Tab::Deep, Opt::WordsOnALine));
        assert!(
            !o.get(Tab::Content, Opt::WholeWord) && !o.get(Tab::Deep, Opt::WholeWord),
            "off, as before"
        );
        assert!(
            o.get(Tab::Content, Opt::Regex) && o.get(Tab::Deep, Opt::Regex),
            "a pattern is a regular expression, as before"
        );
        let old: Options = toml::from_str("[content]\nmatch_case = true\n[deep]\n").unwrap();
        assert!(
            old.content.regex && old.deep.regex,
            "also in a config from before"
        );
    }

    #[test]
    fn searching_versions_has_its_own_options_saved_as_versions() {
        let mut o = Options::default();
        assert_eq!(
            o.versions_list(),
            [
                (Opt::WholeWord, false),
                (Opt::Regex, true),
                (Opt::Binary, false),
                (Opt::Documents, false),
                (Opt::SmartCase, false),
                (Opt::MatchCase, false)
            ],
            "a pattern is Perl's regular expression, binaries and documents left out"
        );
        o.toggle_versions(Opt::Binary);
        o.toggle_versions(Opt::Documents);
        o.toggle_versions(Opt::Regex);
        assert!(o.versions.binary && o.versions.documents && !o.versions.regex);
        assert!(!o.deep.documents, "Deep's Documents stays as it is");
        o.toggle_versions(Opt::Hidden);
        assert_eq!(
            o.versions_list().len(),
            6,
            "an option it does not have does nothing"
        );
        let saved = toml::to_string(&o).unwrap();
        assert!(saved.contains("[versions]\n"), "{saved}");
        let old: Options = toml::from_str("[git]\nall_branches = true\n").unwrap();
        assert_eq!(old.versions, VersionsOpts::default());
        assert_eq!(Opt::Binary.label(Tab::Git), "Binary files");
    }

    #[test]
    fn find_has_its_own_options_all_off_and_saved_as_find() {
        let mut o = Options::default();
        assert_eq!(
            o.find_list(),
            [
                (Opt::WholeWord, false),
                (Opt::Regex, false),
                (Opt::SmartCase, false),
                (Opt::MatchCase, false)
            ]
        );
        o.toggle_find(Opt::WholeWord);
        o.toggle_find(Opt::Regex);
        assert!(o.find.whole_word && o.find.regex && !o.find.match_case);
        o.toggle_find(Opt::MatchCase);
        assert!(o.find.match_case);
        assert!(
            !o.content.match_case && !o.in_file.match_case,
            "Find's case leaves the tabs' alone"
        );
        o.toggle_find(Opt::Hidden);
        assert_eq!(
            o.find_list().len(),
            4,
            "an option Find does not have does nothing"
        );
        let saved = toml::to_string(&o).unwrap();
        assert!(
            saved.contains("[find]\nmatch_case = true\nwhole_word = true\nregex = true\n"),
            "{saved}"
        );
        let old: Options = toml::from_str("[content]\nmatching_lines = true\n").unwrap();
        assert_eq!(
            old.find,
            FindOpts::default(),
            "a config from before has none"
        );
    }

    #[test]
    fn in_a_file_all_words_on_a_line_is_on() {
        let mut o = Options::default();
        assert_eq!(
            o.in_file_list(),
            [
                (Opt::WordsOnALine, true),
                (Opt::WordsInTheFile, false),
                (Opt::SmartCase, false),
                (Opt::MatchCase, false)
            ]
        );
        o.toggle_in_file(Opt::WordsInTheFile);
        assert_eq!(
            o.in_file_list()[..2],
            [(Opt::WordsOnALine, false), (Opt::WordsInTheFile, true)]
        );
        o.toggle(Tab::Content, Opt::WordsOnALine);
        assert!(o.content.words_on_a_line && !o.content.words_in_the_file);
        o.toggle(Tab::Content, Opt::WordsInTheFile);
        assert!(
            !o.content.words_on_a_line && o.content.words_in_the_file,
            "one or the other"
        );
        assert!(
            o.in_file.words_in_the_file,
            "a tab's words leave the file's alone"
        );
        let old: Options = toml::from_str("[content]\nmatching_lines = true\n").unwrap();
        assert!(old.in_file.words_on_a_line && !old.content.match_case);
    }

    #[test]
    fn toggle_changes_only_that_tab() {
        let mut opts = Options::default();
        opts.toggle(Tab::Content, Opt::Hidden);
        assert!(opts.get(Tab::Content, Opt::Hidden));
        assert!(!opts.get(Tab::Files, Opt::Hidden));
        assert!(!opts.get(Tab::Folders, Opt::Hidden));
    }

    #[test]
    fn option_of_another_tab_is_off_and_ignored() {
        let mut opts = Options::default();
        opts.toggle(Tab::Files, Opt::AllBranches);
        assert_eq!(opts, Options::default());
        assert!(!opts.get(Tab::Files, Opt::AllBranches));
    }

    #[test]
    fn folders_labels_say_folders() {
        assert_eq!(Opt::Hidden.label(Tab::Folders), "Hidden folders");
        assert_eq!(Opt::Hidden.label(Tab::Content), "Hidden files");
    }
}
