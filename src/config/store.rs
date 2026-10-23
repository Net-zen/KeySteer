//! Comment-preserving TOML storage with validate-before-swap semantics.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item, Table};

use super::{Config, ConfigError};

pub type ReplaceFile = fn(&Path, &Path) -> std::io::Result<()>;

#[derive(Debug, Clone)]
pub struct ConfigStore {
    path: PathBuf,
    source: StoreSource,
    replace_file: ReplaceFile,
}

#[derive(Debug, Clone)]
enum StoreSource {
    Raw(String),
    Parsed(DocumentMut),
}

impl StoreSource {
    fn text(&self) -> String {
        match self {
            Self::Raw(text) => text.clone(),
            Self::Parsed(document) => document.to_string(),
        }
    }
}

impl ConfigStore {
    pub fn open(
        path: impl Into<PathBuf>,
        fallback: &Config,
        replace_file: ReplaceFile,
    ) -> Result<Self, ConfigError> {
        let path = path.into();
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && std::fs::symlink_metadata(&path)
                        .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                fallback.to_toml()?
            }
            Err(error) => {
                return Err(ConfigError::Io(format!(
                    "cannot read {}: {error}",
                    path.display()
                )));
            }
        };
        Config::parse(&text)?;
        Ok(Self::from_validated_text(path, text, replace_file))
    }

    pub(crate) fn from_validated_text(
        path: impl Into<PathBuf>,
        text: String,
        replace_file: ReplaceFile,
    ) -> Self {
        // The caller already parsed and validated this exact source. Keep the
        // compact text and defer the comment-preserving AST until the first edit.
        Self {
            path: path.into(),
            source: StoreSource::Raw(text),
            replace_file,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Return the exact source currently backing the active configuration.
    ///
    /// This intentionally serializes the comment-preserving document after a
    /// runtime edit instead of rereading the file, so callers see the same
    /// configuration that the engine is using.
    pub(crate) fn source_text(&self) -> String {
        self.source.text()
    }

    pub fn reload(&mut self) -> Result<Config, ConfigError> {
        let text = std::fs::read_to_string(&self.path)
            .map_err(|e| ConfigError::Io(format!("cannot read {}: {e}", self.path.display())))?;
        let config = Config::parse(&text)?;
        config.validate()?;
        self.source = StoreSource::Raw(text);
        Ok(config)
    }

    pub fn set(&mut self, path: &str, raw_value: &str) -> Result<Config, ConfigError> {
        let (candidate, config) = self.prepare_set(path, raw_value)?;
        candidate.persist()?;
        *self = candidate;
        Ok(config)
    }

    /// Build an edited, validated store without touching the active file.
    pub fn prepare_set(&self, path: &str, raw_value: &str) -> Result<(Self, Config), ConfigError> {
        let segments: Vec<&str> = path.split('.').filter(|part| !part.is_empty()).collect();
        if segments.is_empty() {
            return Err(ConfigError::Invalid("config path must not be empty".into()));
        }

        let parsed = format!("value = {raw_value}")
            .parse::<DocumentMut>()
            .map_err(|e| ConfigError::Parse(format!("invalid TOML value for {path}: {e}")))?;
        let value = parsed
            .get("value")
            .cloned()
            .ok_or_else(|| ConfigError::Parse(format!("missing TOML value for {path}")))?;

        let document = match &self.source {
            StoreSource::Raw(text) => text.parse::<DocumentMut>().map_err(|e| {
                ConfigError::Parse(format!("cannot edit {}: {e}", self.path.display()))
            })?,
            StoreSource::Parsed(document) => document.clone(),
        };
        let mut candidate = document;
        set_item(candidate.as_table_mut(), &segments, value)?;
        let text = candidate.to_string();
        let config = Config::parse(&text)?;
        config.validate()?;
        let store = Self {
            path: self.path.clone(),
            source: StoreSource::Parsed(candidate),
            replace_file: self.replace_file,
        };
        Ok((store, config))
    }

    pub fn persist(&self) -> Result<(), ConfigError> {
        atomic_write(&self.path, self.source.text().as_bytes(), self.replace_file).map_err(
            |error| ConfigError::Io(format!("cannot save {}: {error}", self.path.display())),
        )
    }
}

fn set_item(table: &mut Table, path: &[&str], value: Item) -> Result<(), ConfigError> {
    let Some((head, tail)) = path.split_first() else {
        return Err(ConfigError::Invalid("config path must not be empty".into()));
    };
    if tail.is_empty() {
        table.insert(head, value);
        return Ok(());
    }
    if !table.contains_key(head) {
        table.insert(head, Item::Table(Table::new()));
    }
    let child = table
        .get_mut(head)
        .and_then(Item::as_table_mut)
        .ok_or_else(|| ConfigError::Invalid(format!("{head:?} is not a TOML table")))?;
    set_item(child, tail, value)
}

fn atomic_write(path: &Path, bytes: &[u8], replace_file: ReplaceFile) -> io::Result<()> {
    // Resolve at each save while retaining the logical path in ConfigStore for
    // reload. Never fall back to replacing a link when its target is unavailable.
    let (target, permissions) = match fs::canonicalize(path) {
        Ok(target) => {
            let metadata = fs::metadata(&target)?;
            if !metadata.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "config target is not a regular file",
                ));
            }
            let permissions = metadata.permissions();
            if permissions.readonly() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "config target is read-only",
                ));
            }
            (target, Some(permissions))
        }
        Err(error)
            if error.kind() == io::ErrorKind::NotFound
                && fs::symlink_metadata(path)
                    .is_err_and(|error| error.kind() == io::ErrorKind::NotFound) =>
        {
            (path.to_path_buf(), None)
        }
        Err(error) => return Err(error),
    };
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temp = target.with_added_extension(format!("{}.tmp", std::process::id()));
    let mut file = File::create_new(&temp)?;
    let result = (|| -> io::Result<()> {
        file.write_all(bytes)?;
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)?;
        }
        file.sync_all()?;
        drop(file);
        replace_file(&temp, &target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::tests::filesystem::{TestDirectory, symlink_dir, symlink_file};

    #[test]
    fn creates_and_updates_configs_through_a_directory_symlink() {
        let directory = TestDirectory::new("directory-linked-config-store");
        let target_directory = directory.0.join("dotfiles");
        std::fs::create_dir(&target_directory).unwrap();
        let alias = directory.0.join("profiles");
        symlink_dir(&target_directory, &alias);
        let logical_path = alias.join("keysteer.mine.toml");
        let real_path = target_directory.join("keysteer.mine.toml");
        let mut store = ConfigStore::open(
            &logical_path,
            &Config::default(),
            crate::platform::atomic_replace,
        )
        .unwrap();

        for speed in [17, 33] {
            store
                .set("pointer.initial_speed", &speed.to_string())
                .unwrap();
            assert_eq!(
                Config::load(&real_path).unwrap().pointer.initial_speed,
                f64::from(speed)
            );
            assert_eq!(store.path(), logical_path);
            assert_eq!(std::fs::read_link(&alias).unwrap(), target_directory);
            assert_eq!(std::fs::read_dir(&target_directory).unwrap().count(), 1);
        }
    }

    #[test]
    fn symlink_chain_saves_preserve_links_comments_and_follow_retargeting_on_reload() {
        let directory = TestDirectory::new("linked-config-store");
        let app = directory.0.join("app");
        let dotfiles = directory.0.join("dotfiles");
        std::fs::create_dir(&app).unwrap();
        std::fs::create_dir(&dotfiles).unwrap();
        let target = dotfiles.join("real.toml");
        std::fs::write(
            &target,
            "# dotfiles comment\n[pointer]\ninitial_speed = 33\n",
        )
        .unwrap();
        let intermediate = dotfiles.join("config.link");
        symlink_file(&target, &intermediate);
        let link = app.join("keysteer.mine.toml");
        let relative = Path::new("..").join("dotfiles").join("config.link");
        symlink_file(&relative, &link);
        let mut store =
            ConfigStore::open(&link, &Config::default(), crate::platform::atomic_replace).unwrap();

        store.set("pointer.initial_speed", "17").unwrap();
        assert_eq!(store.path(), link);
        assert_eq!(std::fs::read_link(&link).unwrap(), relative);
        assert_eq!(std::fs::read_link(&intermediate).unwrap(), target);
        let saved = std::fs::read_to_string(&target).unwrap();
        assert!(saved.contains("# dotfiles comment"));
        assert_eq!(Config::parse(&saved).unwrap().pointer.initial_speed, 17.0);

        std::fs::write(&target, "[pointer]\ninitial_speed = 34\n").unwrap();
        assert_eq!(store.reload().unwrap().pointer.initial_speed, 34.0);
        let new_target = dotfiles.join("other.toml");
        std::fs::write(&new_target, "[pointer]\ninitial_speed = 50\n").unwrap();
        std::fs::remove_file(&intermediate).unwrap();
        symlink_file(&new_target, &intermediate);
        assert_eq!(store.reload().unwrap().pointer.initial_speed, 50.0);
        store.set("pointer.initial_speed", "51").unwrap();
        assert_eq!(
            Config::load(&new_target).unwrap().pointer.initial_speed,
            51.0
        );
        assert_eq!(Config::load(&target).unwrap().pointer.initial_speed, 34.0);
        assert_eq!(std::fs::read_link(&link).unwrap(), relative);
        assert_eq!(std::fs::read_link(&intermediate).unwrap(), new_target);
        assert_eq!(std::fs::read_dir(&app).unwrap().count(), 1);
        assert_eq!(std::fs::read_dir(&dotfiles).unwrap().count(), 3);
    }

    #[test]
    fn broken_symlink_save_does_not_create_a_target_or_replace_the_link() {
        let directory = TestDirectory::new("broken-config-store");
        let target = directory.0.join("real.toml");
        std::fs::write(&target, "[pointer]\ninitial_speed = 33\n").unwrap();
        let link = directory.0.join("keysteer.mine.toml");
        symlink_file(Path::new("real.toml"), &link);
        let mut store =
            ConfigStore::open(&link, &Config::default(), crate::platform::atomic_replace).unwrap();
        let before = store.source_text();
        std::fs::remove_file(&target).unwrap();

        assert!(
            ConfigStore::open(&link, &Config::default(), crate::platform::atomic_replace).is_err()
        );
        assert!(store.set("pointer.initial_speed", "17").is_err());
        assert_eq!(store.source_text(), before);
        assert_eq!(std::fs::read_link(&link).unwrap(), Path::new("real.toml"));
        assert!(!target.exists());
        assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 1);
    }

    #[test]
    fn read_only_symlink_target_keeps_both_the_file_and_active_document() {
        let directory = TestDirectory::new("readonly-config-store");
        let target = directory.0.join("real.toml");
        let original = "[pointer]\ninitial_speed = 33\n";
        std::fs::write(&target, original).unwrap();
        let permissions = std::fs::metadata(&target).unwrap().permissions();
        let mut readonly = permissions.clone();
        readonly.set_readonly(true);
        std::fs::set_permissions(&target, readonly).unwrap();
        let link = directory.0.join("keysteer.mine.toml");
        symlink_file(&target, &link);
        let mut store =
            ConfigStore::open(&link, &Config::default(), crate::platform::atomic_replace).unwrap();
        let result = store.set("pointer.initial_speed", "17");
        std::fs::set_permissions(&target, permissions).unwrap();

        assert!(result.unwrap_err().to_string().contains("read-only"));
        assert_eq!(store.source_text(), original);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), original);
        assert_eq!(std::fs::read_link(&link).unwrap(), target);
    }

    #[test]
    fn failed_replacement_cleans_up_a_temporary_file_beside_the_real_target() {
        fn fail(source: &Path, destination: &Path) -> std::io::Result<()> {
            assert_eq!(source.parent(), destination.parent());
            assert_eq!(destination.file_name().unwrap(), "real.toml");
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "replacement denied",
            ))
        }

        let directory = TestDirectory::new("failed-linked-config-store");
        let dotfiles = directory.0.join("dotfiles");
        std::fs::create_dir(&dotfiles).unwrap();
        let target = dotfiles.join("real.toml");
        let original = "[pointer]\ninitial_speed = 33\n";
        std::fs::write(&target, original).unwrap();
        let occupied = target.with_added_extension(format!("{}.tmp", std::process::id()));
        std::fs::write(&occupied, "another pending save").unwrap();
        let link = directory.0.join("keysteer.mine.toml");
        symlink_file(&target, &link);
        let mut store = ConfigStore::open(&link, &Config::default(), fail).unwrap();
        assert!(store.set("pointer.initial_speed", "17").is_err());
        assert_eq!(
            std::fs::read_to_string(&occupied).unwrap(),
            "another pending save"
        );
        std::fs::remove_file(&occupied).unwrap();
        assert!(
            store
                .set("pointer.initial_speed", "17")
                .unwrap_err()
                .to_string()
                .contains("replacement denied")
        );

        assert_eq!(store.source_text(), original);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), original);
        assert_eq!(std::fs::read_link(&link).unwrap(), target);
        assert_eq!(std::fs::read_dir(&dotfiles).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn replacement_preserves_existing_target_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let directory = TestDirectory::new("config-store-permissions");
        let path = directory.0.join("keysteer.mine.toml");
        std::fs::write(&path, "[pointer]\ninitial_speed = 33\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut store =
            ConfigStore::open(&path, &Config::default(), crate::platform::atomic_replace).unwrap();
        store.set("pointer.initial_speed", "17").unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
        if destination.exists() {
            std::fs::remove_file(destination)?;
        }
        std::fs::rename(source, destination)
    }

    fn test_file_name(prefix: &str) -> String {
        let current_thread = std::thread::current();
        let thread_name = current_thread.name().unwrap_or("test");
        let sanitized: String = thread_name
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                    character
                } else {
                    '_'
                }
            })
            .collect();
        format!("{prefix}-{}-{sanitized}.toml", std::process::id())
    }

    #[test]
    fn creates_nested_platform_setting_tables() {
        let path = std::env::temp_dir().join(test_file_name("keysteer-platform-store"));
        let mut store = ConfigStore::open(&path, &Config::default(), replace_file).unwrap();
        let config = store
            .set("platform.macos.scroll.invert_vertical", "false")
            .unwrap();
        assert_eq!(config.platform.macos.scroll.invert_vertical, Some(false));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn invalid_update_keeps_the_last_valid_document() {
        let path = std::env::temp_dir().join(test_file_name("keysteer-store"));
        let mut store = ConfigStore::open(&path, &Config::default(), replace_file).unwrap();
        let before = store.source.text();
        assert!(store.set("pointer.initial_speed", "0").is_err());
        assert_eq!(store.source.text(), before);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn prepared_update_does_not_touch_disk_until_committed() {
        let path = std::env::temp_dir().join(test_file_name("keysteer-prepared-store"));
        let original = Config::default().to_toml().unwrap();
        std::fs::write(&path, &original).unwrap();
        let mut store = ConfigStore::open(&path, &Config::default(), replace_file).unwrap();
        for speed in ["17", "23"] {
            let before = store.source_text();
            let (candidate, config) = store.prepare_set("pointer.initial_speed", speed).unwrap();
            assert_eq!(config.pointer.initial_speed, speed.parse::<f64>().unwrap());
            assert_eq!(store.source_text(), before);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), before);

            candidate.persist().unwrap();
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                candidate.source_text()
            );
            store = candidate;
        }
        assert_ne!(store.source_text(), original);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn updates_platform_specific_key_aliases() {
        let path =
            std::env::temp_dir().join(format!("keysteer-alias-store-{}.toml", std::process::id()));
        let mut store = ConfigStore::open(&path, &Config::default(), replace_file).unwrap();
        let config = store
            .set("key_aliases.windows.Primary", "\"left_alt\"")
            .unwrap();
        assert_eq!(config.key_aliases.windows["Primary"], "left_alt");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn document_ast_is_lazy_and_reload_returns_to_raw_source() {
        let path = std::env::temp_dir().join(test_file_name("keysteer-lazy-store"));
        std::fs::write(&path, Config::default().to_toml().unwrap()).unwrap();
        let mut store = ConfigStore::open(&path, &Config::default(), replace_file).unwrap();
        assert!(matches!(store.source, StoreSource::Raw(_)));
        store.set("pointer.initial_speed", "11").unwrap();
        assert!(matches!(store.source, StoreSource::Parsed(_)));
        store.reload().unwrap();
        assert!(matches!(store.source, StoreSource::Raw(_)));
        let _ = std::fs::remove_file(path);
    }
}
