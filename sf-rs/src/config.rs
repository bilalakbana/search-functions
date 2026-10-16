//! The options remembered across runs, in `$XDG_CONFIG_HOME/sf/config.toml`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::model::Options;

/// `$XDG_CONFIG_HOME/sf/config.toml`, else `~/.config/sf/config.toml`.
pub fn default_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("sf").join("config.toml"))
}

/// A missing file gives the defaults; an unreadable or invalid one is an error (and is not touched).
pub fn load(path: &Path) -> Result<Options, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Options::default()),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    toml::from_str(&text).map_err(|e| {
        let line = e.span().map(|s| text[..s.start].matches('\n').count() + 1);
        let at = line.map(|n| format!(" (line {n})")).unwrap_or_default();
        format!("{}{at}: {}", path.display(), e.message())
    })
}

/// Writes the options, creating the folder when needed.
pub fn save(path: &Path, opts: &Options) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let text = toml::to_string(opts).context("encoding the options")?;
    fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Opt, Tab};
    use crate::testutil::temp_dir;
    use std::fs;

    #[test]
    fn defaults_match_spec() {
        let o = Options::default();
        let on = |tab, opt| o.get(tab, opt);
        // Bilal, 2026-10-17: hidden off, ignored on, in every tab that has them
        for tab in [Tab::Files, Tab::Content, Tab::Folders] {
            assert!(!on(tab, Opt::Hidden), "{tab:?} hidden");
            assert!(on(tab, Opt::Ignored), "{tab:?} ignored");
        }
        assert!(!on(Tab::Content, Opt::MatchingLines));
        assert!(!on(Tab::Content, Opt::Archives));
        assert!(on(Tab::Git, Opt::AllBranches));
    }

    #[test]
    fn round_trip() {
        let path = temp_dir("config_round_trip").join("sub/config.toml");
        let mut o = Options::default();
        o.toggle(Tab::Content, Opt::MatchingLines);
        o.toggle(Tab::Git, Opt::AllBranches);
        save(&path, &o).unwrap();
        assert_eq!(load(&path).unwrap(), o);
    }

    #[test]
    fn missing_file_gives_defaults() {
        let path = temp_dir("config_missing").join("config.toml");
        assert_eq!(load(&path).unwrap(), Options::default());
    }

    #[test]
    fn invalid_file_is_an_error_and_untouched() {
        let path = temp_dir("config_invalid").join("config.toml");
        let bytes = b"[search\nmatching_lines = yes\n";
        fs::write(&path, bytes).unwrap();
        let err = load(&path).unwrap_err();
        assert!(err.contains("config.toml"), "{err}");
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn wrong_type_is_an_error() {
        let path = temp_dir("config_wrong_type").join("config.toml");
        fs::write(&path, "[files]\nhidden = \"yes\"\n").unwrap();
        let err = load(&path).unwrap_err();
        assert!(err.contains("(line 2)"), "{err}");
    }

    #[test]
    fn a_config_from_when_content_was_called_search_still_loads() {
        let path = temp_dir("config_old_name").join("config.toml");
        fs::write(&path, "[search]\nmatching_lines = true\n").unwrap();
        assert!(load(&path).unwrap().get(Tab::Content, Opt::MatchingLines));
    }

    #[test]
    fn partial_file_fills_defaults() {
        let path = temp_dir("config_partial").join("config.toml");
        fs::write(&path, "[search]\nmatching_lines = true\n").unwrap();
        let mut want = Options::default();
        want.content.matching_lines = true;
        assert_eq!(load(&path).unwrap(), want);
    }
}
