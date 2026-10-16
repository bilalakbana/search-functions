//! The tabs, their options and the defaults the spec sets for them.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Files,
    Search,
    Folders,
    Git,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Files, Tab::Search, Tab::Folders, Tab::Git];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Files => "Files",
            Tab::Search => "Search",
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

    /// "files", "Search", ... (the command-line argument).
    pub fn from_name(name: &str) -> Option<Tab> {
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
pub struct SearchOpts {
    pub matching_lines: bool,
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
            hidden: true,
            ignored: true,
        }
    }
}

impl Default for SearchOpts {
    fn default() -> Self {
        SearchOpts {
            matching_lines: false,
            archives: false,
            hidden: true,
            ignored: false,
        }
    }
}

impl Default for FoldersOpts {
    fn default() -> Self {
        FoldersOpts {
            hidden: true,
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
    pub search: SearchOpts,
    pub folders: FoldersOpts,
    pub git: GitOpts,
}

impl Options {
    /// The tab's options in popup order, with their values.
    pub fn list(&self, tab: Tab) -> Vec<(Opt, bool)> {
        let order: &[Opt] = match tab {
            Tab::Files | Tab::Folders => &[Opt::Hidden, Opt::Ignored],
            Tab::Search => &[Opt::MatchingLines, Opt::Archives, Opt::Hidden, Opt::Ignored],
            Tab::Git => &[Opt::AllBranches],
        };
        order.iter().map(|&o| (o, self.get(tab, o))).collect()
    }

    /// An option the tab does not have is off.
    pub fn get(&self, tab: Tab, opt: Opt) -> bool {
        self.clone().slot(tab, opt).is_some_and(|v| *v)
    }

    /// Toggling an option the tab does not have does nothing.
    pub fn toggle(&mut self, tab: Tab, opt: Opt) {
        if let Some(v) = self.slot(tab, opt) {
            *v = !*v;
        }
    }

    fn slot(&mut self, tab: Tab, opt: Opt) -> Option<&mut bool> {
        match (tab, opt) {
            (Tab::Files, Opt::Hidden) => Some(&mut self.files.hidden),
            (Tab::Files, Opt::Ignored) => Some(&mut self.files.ignored),
            (Tab::Search, Opt::MatchingLines) => Some(&mut self.search.matching_lines),
            (Tab::Search, Opt::Archives) => Some(&mut self.search.archives),
            (Tab::Search, Opt::Hidden) => Some(&mut self.search.hidden),
            (Tab::Search, Opt::Ignored) => Some(&mut self.search.ignored),
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
        assert_eq!(Tab::Files.next(), Tab::Search);
        assert_eq!(Tab::Search.prev(), Tab::Files);
    }

    #[test]
    fn from_name_ignores_case() {
        assert_eq!(Tab::from_name("search"), Some(Tab::Search));
        assert_eq!(Tab::from_name("Git"), Some(Tab::Git));
        assert_eq!(Tab::from_name("content"), None);
    }

    #[test]
    fn search_popup_order() {
        let opts = Options::default();
        let order: Vec<Opt> = opts.list(Tab::Search).into_iter().map(|(o, _)| o).collect();
        assert_eq!(
            order,
            [Opt::MatchingLines, Opt::Archives, Opt::Hidden, Opt::Ignored]
        );
    }

    #[test]
    fn toggle_changes_only_that_tab() {
        let mut opts = Options::default();
        opts.toggle(Tab::Search, Opt::Hidden);
        assert!(!opts.get(Tab::Search, Opt::Hidden));
        assert!(opts.get(Tab::Files, Opt::Hidden));
        assert!(opts.get(Tab::Folders, Opt::Hidden));
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
        assert_eq!(Opt::Hidden.label(Tab::Search), "Hidden files");
    }
}
