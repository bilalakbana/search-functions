//! The tabs, their options and the defaults the spec sets for them.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Files,
    Content,
    Folders,
    Git,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Files, Tab::Content, Tab::Folders, Tab::Git];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Files => "Files",
            Tab::Content => "Content",
            Tab::Folders => "Folders",
            Tab::Git => "Git",
        }
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
    AllBranches,
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
            Opt::AllBranches => "All branches",
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
            Opt::AllBranches => "not only the current one",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FilesOpts {
    pub hidden: bool,
    pub ignored: bool,
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
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FoldersOpts {
    pub hidden: bool,
    pub ignored: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GitOpts {
    pub all_branches: bool,
}

impl Default for FilesOpts {
    fn default() -> Self {
        FilesOpts {
            hidden: false,
            ignored: true,
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
        }
    }
}

impl Default for FoldersOpts {
    fn default() -> Self {
        FoldersOpts {
            hidden: false,
            ignored: true,
        }
    }
}

impl Default for GitOpts {
    fn default() -> Self {
        GitOpts { all_branches: true }
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
    pub folders: FoldersOpts,
    pub git: GitOpts,
}

impl Options {
    /// The tab's options in popup order, with their values.
    pub fn list(&self, tab: Tab) -> Vec<(Opt, bool)> {
        let order: &[Opt] = match tab {
            Tab::Files | Tab::Folders => &[Opt::Hidden, Opt::Ignored],
            Tab::Content => &[
                Opt::MatchingLines,
                Opt::EachMatch,
                Opt::Archives,
                Opt::Hidden,
                Opt::Ignored,
            ],
            Tab::Git => &[Opt::AllBranches],
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
        if *v {
            match opt {
                Opt::MatchingLines => self.content.each_match = false,
                Opt::EachMatch => self.content.matching_lines = false,
                _ => {}
            }
        }
    }

    fn slot(&mut self, tab: Tab, opt: Opt) -> Option<&mut bool> {
        match (tab, opt) {
            (Tab::Files, Opt::Hidden) => Some(&mut self.files.hidden),
            (Tab::Files, Opt::Ignored) => Some(&mut self.files.ignored),
            (Tab::Content, Opt::MatchingLines) => Some(&mut self.content.matching_lines),
            (Tab::Content, Opt::EachMatch) => Some(&mut self.content.each_match),
            (Tab::Content, Opt::Archives) => Some(&mut self.content.archives),
            (Tab::Content, Opt::Hidden) => Some(&mut self.content.hidden),
            (Tab::Content, Opt::Ignored) => Some(&mut self.content.ignored),
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
        assert_eq!(Tab::Git.next(), Tab::Files);
        assert_eq!(Tab::Files.prev(), Tab::Git);
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
                Opt::Ignored
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
