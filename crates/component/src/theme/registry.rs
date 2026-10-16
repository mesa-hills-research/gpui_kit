use crate::{Theme, ThemeColor, ThemeConfig, ThemeMode, ThemeSet, highlighter::HighlightTheme};
#[allow(unused)]
use anyhow::Result;
use gpui::{App, Global, SharedString};
use std::{
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, LazyLock},
};

const DEFAULT_THEME: &str = include_str!("./default-theme.json");
pub(crate) static DEFAULT_THEME_COLORS: LazyLock<
    HashMap<ThemeMode, (Arc<ThemeColor>, Arc<HighlightTheme>)>,
> = LazyLock::new(|| {
    let mut colors = HashMap::new();

    let themes: Vec<ThemeConfig> = serde_json::from_str::<ThemeSet>(DEFAULT_THEME)
        .expect("Failed to parse themes/default.json")
        .themes;

    for theme in themes {
        let mut theme_color = ThemeColor::default();
        theme_color.apply_config(&theme, &ThemeColor::default());

        let highlight_theme = HighlightTheme {
            name: theme.name.to_string(),
            appearance: theme.mode,
            style: theme.highlight.unwrap_or_default(),
        };

        colors.insert(
            theme.mode,
            (Arc::new(theme_color), Arc::new(highlight_theme)),
        );
    }

    colors
});

pub(super) fn init(cx: &mut App) {
    cx.set_global(ThemeRegistry::default());
    ThemeRegistry::global_mut(cx).init_default_themes();

    // Observe changes to the theme registry to apply changes to the active theme
    cx.observe_global::<ThemeRegistry>(|cx| {
        let mode = Theme::global(cx).mode;
        let light_theme = Theme::global(cx).light_theme.name.clone();
        let dark_theme = Theme::global(cx).dark_theme.name.clone();

        if let Some(theme) = ThemeRegistry::global(cx).theme(&light_theme).cloned() {
            Theme::global_mut(cx).light_theme = theme;
        }
        if let Some(theme) = ThemeRegistry::global(cx).theme(&dark_theme).cloned() {
            Theme::global_mut(cx).dark_theme = theme;
        }

        let theme_name = if mode.is_dark() {
            dark_theme
        } else {
            light_theme
        };

        tracing::info!("Reload active theme: {:?}...", theme_name);
        Theme::change(mode, None, cx);
        cx.refresh_windows();
    })
    .detach();
}

/// Every theme an application can switch to, by name.
///
/// Themes come from three places, later ones replacing earlier ones of the
/// same name:
///
/// 1. the kit's two defaults;
/// 2. themes the application embeds in its binary, added with
///    [`ThemeRegistry::add_bundled_themes`];
/// 3. the folder [`ThemeRegistry::watch_dir`] watches, the user's own
///    themes, which replace a bundled theme whose name matches ignoring
///    case. The kit's defaults are never replaced.
///
/// The folder is read again whenever a file in it changes; bundled themes
/// stay registered across those reloads.
#[derive(Default, Debug)]
pub struct ThemeRegistry {
    themes_dir: PathBuf,
    default_themes: HashMap<ThemeMode, Rc<ThemeConfig>>,
    /// Themes embedded in the application, in the order they were added.
    bundled_themes: Vec<Rc<ThemeConfig>>,
    /// The themes read from `themes_dir` at its last load.
    folder_themes: Vec<Rc<ThemeConfig>>,
    themes: HashMap<SharedString, Rc<ThemeConfig>>,
    has_custom_themes: bool,
}

impl Global for ThemeRegistry {}

impl ThemeRegistry {
    pub fn global(cx: &App) -> &Self {
        cx.global::<Self>()
    }

    pub fn global_mut(cx: &mut App) -> &mut Self {
        cx.global_mut::<Self>()
    }

    /// Watch a folder of theme files, such as the user's own themes.
    ///
    /// Its `*.json` files are read and parsed on a background thread, now
    /// and whenever a file in it changes, and replace the bundled themes of
    /// the same name (ignoring case). `on_load` runs once, after the first
    /// read. The folder is created if it is missing.
    #[cfg(not(target_family = "wasm"))]
    pub fn watch_dir<F>(themes_dir: PathBuf, cx: &mut App, on_load: F) -> Result<()>
    where
        F: Fn(&mut App) + 'static,
    {
        Self::global_mut(cx).themes_dir = themes_dir.clone();

        cx.spawn(async move |cx| {
            let dir = themes_dir.clone();
            let themes = cx
                .background_executor()
                .spawn(async move { read_themes_dir(&dir) })
                .await;
            _ = cx.update(|cx| {
                if let Err(err) = Self::_watch_themes_dir(themes_dir, cx) {
                    tracing::error!("Failed to watch themes directory: {}", err);
                }

                Self::global_mut(cx).set_folder_themes(themes);
                on_load(cx);
            });
        })
        .detach();

        Ok(())
    }

    /// Add themes the application embeds in its binary.
    ///
    /// They stay registered when the watched folder reloads, and a theme in
    /// that folder with the same name, ignoring case, replaces one of them.
    /// A bundled theme replaces an earlier bundled theme of the same name.
    pub fn add_bundled_themes(&mut self, themes: impl IntoIterator<Item = ThemeConfig>) {
        for theme in themes {
            self.bundled_themes
                .retain(|bundled| !same_name(&bundled.name, &theme.name));
            self.bundled_themes.push(Rc::new(theme));
        }
        self.rebuild();
    }

    /// Parse a theme file (a `ThemeSet`, as in the kit's `themes` folder)
    /// embedded in the application, and add its themes as with
    /// [`ThemeRegistry::add_bundled_themes`].
    ///
    /// ```ignore
    /// ThemeRegistry::global_mut(cx)
    ///     .add_bundled_themes_from_str(include_str!("../themes/macos-classic.json"))?;
    /// ```
    pub fn add_bundled_themes_from_str(&mut self, content: &str) -> anyhow::Result<()> {
        let theme_set = serde_json::from_str::<ThemeSet>(content)?;
        self.add_bundled_themes(theme_set.themes);
        Ok(())
    }

    /// The theme named `name`: an exact match, else one whose name matches
    /// ignoring case, so a user's theme that replaced a bundled one under a
    /// different spelling is still found by the bundled name.
    pub fn theme(&self, name: &str) -> Option<&Rc<ThemeConfig>> {
        self.themes.get(name).or_else(|| {
            self.themes
                .iter()
                .find(|(key, _)| same_name(key, name))
                .map(|(_, theme)| theme)
        })
    }

    /// Returns a reference to the map of themes (including default themes).
    pub fn themes(&self) -> &HashMap<SharedString, Rc<ThemeConfig>> {
        &self.themes
    }

    /// Returns a sorted list of themes.
    pub fn sorted_themes(&self) -> Vec<&Rc<ThemeConfig>> {
        let mut themes = self.themes.values().collect::<Vec<_>>();
        // sort by is_default true first, then light first dark later, then by name case-insensitive
        themes.sort_by(|a, b| {
            b.is_default
                .cmp(&a.is_default)
                .then(a.mode.cmp(&b.mode))
                .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        themes
    }

    /// Returns a reference to the map of default themes.
    pub fn default_themes(&self) -> &HashMap<ThemeMode, Rc<ThemeConfig>> {
        &self.default_themes
    }

    pub fn default_light_theme(&self) -> &Rc<ThemeConfig> {
        &self.default_themes[&ThemeMode::Light]
    }

    pub fn default_dark_theme(&self) -> &Rc<ThemeConfig> {
        &self.default_themes[&ThemeMode::Dark]
    }

    /// Parse a theme file and register its themes whose names are not taken.
    ///
    /// These themes last until the watched folder next loads; use
    /// [`ThemeRegistry::add_bundled_themes_from_str`] for themes that must
    /// stay.
    pub fn load_themes_from_str(&mut self, content: &str) -> anyhow::Result<()> {
        let theme_set = serde_json::from_str::<ThemeSet>(content)?;
        for theme in theme_set.themes {
            if !self.themes.contains_key(&theme.name) {
                let theme_name = theme.name.clone();
                self.themes.insert(theme_name, Rc::new(theme));
                self.has_custom_themes = true;
            }
        }
        Ok(())
    }

    fn init_default_themes(&mut self) {
        let default_themes: Vec<ThemeConfig> = serde_json::from_str::<ThemeSet>(DEFAULT_THEME)
            .expect("failed to parse default theme.")
            .themes;
        for theme in default_themes.into_iter() {
            if theme.mode.is_dark() {
                self.default_themes.insert(ThemeMode::Dark, Rc::new(theme));
            } else {
                self.default_themes.insert(ThemeMode::Light, Rc::new(theme));
            }
        }
        self.themes_dir = PathBuf::from("./themes");
        self.themes = self
            .default_themes
            .values()
            .map(|theme| {
                let name = theme.name.clone();
                (name, Rc::clone(theme))
            })
            .collect();
    }

    #[cfg(not(target_family = "wasm"))]
    fn _watch_themes_dir(themes_dir: PathBuf, cx: &mut App) -> anyhow::Result<()> {
        use notify::Watcher as _;

        if !themes_dir.exists() {
            std::fs::create_dir_all(&themes_dir)?;
        }

        let (tx, rx) = smol::channel::bounded(1);
        let mut watcher = notify::RecommendedWatcher::new(
            move |res: notify::Result<notify::Event>| match res {
                Ok(event) => {
                    let touched = event.need_rescan()
                        || matches!(
                            event.kind,
                            notify::EventKind::Any
                                | notify::EventKind::Create(_)
                                | notify::EventKind::Modify(_)
                                | notify::EventKind::Remove(_)
                        );
                    if touched {
                        let _ = tx.try_send(());
                    }
                }
                Err(err) => tracing::error!("Failed to receive theme event: {err}"),
            },
            notify::Config::default().with_follow_symlinks(false),
        )?;

        cx.spawn(async move |cx| {
            if let Err(err) = watcher.watch(&themes_dir, notify::RecursiveMode::NonRecursive) {
                match err.kind {
                    notify::ErrorKind::MaxFilesWatch => tracing::error!(
                        "Theme file watch limit reached, theme hot reload is disabled: {err}"
                    ),
                    _ => tracing::error!("Failed to watch themes directory: {err}"),
                }
            }

            while rx.recv().await.is_ok() {
                tracing::info!("Reloading themes...");
                let dir = themes_dir.clone();
                let themes = cx
                    .background_executor()
                    .spawn(async move { read_themes_dir(&dir) })
                    .await;
                _ = cx.update(|cx| Self::global_mut(cx).set_folder_themes(themes));
            }
        })
        .detach();

        Ok(())
    }

    /// Replace the themes read from the watched folder.
    #[cfg(not(target_family = "wasm"))]
    fn set_folder_themes(&mut self, themes: Vec<ThemeConfig>) {
        self.folder_themes = themes.into_iter().map(Rc::new).collect();
        self.rebuild();
    }

    /// Compose the registered themes from the defaults, the bundled themes
    /// and the folder's themes, in that order of precedence from lowest.
    fn rebuild(&mut self) {
        // A folder theme marked default replaces the kit's default for its
        // mode, and keeps that place while it stays in the folder.
        for theme in &self.folder_themes {
            if theme.is_default {
                self.default_themes.insert(theme.mode, theme.clone());
            }
        }

        self.themes.clear();
        for theme in self.default_themes.values() {
            self.themes.insert(theme.name.clone(), theme.clone());
        }
        let defaults = self
            .default_themes
            .values()
            .map(|theme| theme.name.clone())
            .collect::<Vec<_>>();
        let is_default =
            |name: &SharedString| defaults.iter().any(|default| same_name(default, name));

        for theme in self.bundled_themes.iter().chain(&self.folder_themes) {
            if is_default(&theme.name) {
                continue;
            }
            self.themes.retain(|name, _| !same_name(name, &theme.name));
            self.themes.insert(theme.name.clone(), theme.clone());
        }
        self.has_custom_themes = self.themes.len() > self.default_themes.len();
    }
}

/// Whether two theme names are the same, ignoring case.
fn same_name(a: &str, b: &str) -> bool {
    a.chars()
        .flat_map(char::to_lowercase)
        .eq(b.chars().flat_map(char::to_lowercase))
}

/// Read and parse every `*.json` theme file in `dir`, in file name order,
/// skipping (and logging) files that fail to parse. Runs off the UI thread.
#[cfg(not(target_family = "wasm"))]
fn read_themes_dir(dir: &std::path::Path) -> Vec<ThemeConfig> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            if dir.exists() {
                tracing::error!("Failed to read themes directory {}: {err}", dir.display());
            }
            return Vec::new();
        }
    };
    let mut paths = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    paths.sort();

    let mut themes = Vec::new();
    for path in paths {
        let parsed = std::fs::read_to_string(&path)
            .map_err(anyhow::Error::from)
            .and_then(|content| Ok(serde_json::from_str::<ThemeSet>(&content)?));
        match parsed {
            Ok(theme_set) => themes.extend(theme_set.themes),
            Err(err) => {
                tracing::error!("ignored invalid theme file: {}, {}", path.display(), err)
            }
        }
    }
    themes
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    fn theme_set(themes: &[(&str, &str, &str)]) -> String {
        let themes = themes
            .iter()
            .map(|(name, mode, background)| {
                serde_json::json!({
                    "name": name,
                    "mode": mode,
                    "colors": { "background": background }
                })
            })
            .collect::<Vec<_>>();
        serde_json::json!({ "name": "Set", "author": "Test", "themes": themes }).to_string()
    }

    fn background(registry: &ThemeRegistry, name: &str) -> Option<String> {
        registry
            .theme(name)
            .and_then(|theme| theme.colors.background.clone())
            .map(|color| color.to_string())
    }

    /// A temporary folder of theme files, removed when dropped.
    struct ThemesDir(PathBuf);

    impl ThemesDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "gpui-component-themes-{}-{name}",
                std::process::id()
            ));
            _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn write(&self, file: &str, content: &str) {
            std::fs::write(self.0.join(file), content).unwrap();
        }
    }

    impl Drop for ThemesDir {
        fn drop(&mut self) {
            _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn bundled_themes_survive_folder_loads_and_yield_to_the_users() {
        let mut registry = ThemeRegistry::default();
        registry.init_default_themes();
        registry
            .add_bundled_themes_from_str(&theme_set(&[
                ("Classic Dark", "dark", "#111111"),
                ("Classic Light", "light", "#eeeeee"),
            ]))
            .unwrap();
        assert_eq!(
            background(&registry, "Classic Dark").as_deref(),
            Some("#111111")
        );

        // The user's folder replaces a bundled theme whose name matches
        // ignoring case, and adds its own.
        registry.set_folder_themes(
            serde_json::from_str::<ThemeSet>(&theme_set(&[
                ("classic dark", "dark", "#222222"),
                ("Mine", "light", "#fafafa"),
                ("Default Light", "light", "#000000"),
            ]))
            .unwrap()
            .themes,
        );
        assert_eq!(
            background(&registry, "Classic Dark").as_deref(),
            Some("#222222")
        );
        assert_eq!(
            background(&registry, "classic dark").as_deref(),
            Some("#222222")
        );
        assert_eq!(background(&registry, "Mine").as_deref(), Some("#fafafa"));
        assert_eq!(
            background(&registry, "Classic Light").as_deref(),
            Some("#eeeeee")
        );
        assert!(
            registry.themes().keys().all(|name| name != "Classic Dark"),
            "one theme per name"
        );
        // The kit's defaults are not replaced.
        assert_ne!(
            background(&registry, "Default Light").as_deref(),
            Some("#000000")
        );

        // The next load of the folder drops the user's themes, not the
        // bundled ones.
        registry.set_folder_themes(Vec::new());
        assert_eq!(
            background(&registry, "Classic Dark").as_deref(),
            Some("#111111")
        );
        assert_eq!(background(&registry, "Mine"), None);
        assert_eq!(registry.themes().len(), 4);

        // A bundled theme added later replaces an earlier one of its name.
        registry
            .add_bundled_themes_from_str(&theme_set(&[("CLASSIC DARK", "dark", "#333333")]))
            .unwrap();
        assert_eq!(
            background(&registry, "Classic Dark").as_deref(),
            Some("#333333")
        );
        assert_eq!(registry.themes().len(), 4);
    }

    #[test]
    fn reads_each_valid_theme_file_in_the_folder() {
        let dir = ThemesDir::new("read");
        dir.write("b.json", &theme_set(&[("B", "dark", "#222222")]));
        dir.write("a.json", &theme_set(&[("A", "light", "#111111")]));
        dir.write("broken.json", "{ not json");
        dir.write("notes.txt", "not a theme");

        let names = read_themes_dir(&dir.0)
            .into_iter()
            .map(|theme| theme.name)
            .collect::<Vec<_>>();
        assert_eq!(names, vec![SharedString::from("A"), "B".into()]);
        assert!(read_themes_dir(&dir.0.join("missing")).is_empty());
    }

    #[gpui::test]
    fn watching_a_folder_loads_it_and_keeps_the_active_theme(cx: &mut TestAppContext) {
        let dir = ThemesDir::new("watch");
        dir.write("mine.json", &theme_set(&[("Mine Dark", "dark", "#123456")]));
        cx.update(|cx| {
            crate::theme::init(cx);
            ThemeRegistry::global_mut(cx)
                .add_bundled_themes_from_str(&theme_set(&[("Bundled", "light", "#eeeeee")]))
                .unwrap();
            ThemeRegistry::watch_dir(dir.0.clone(), cx, |_| {}).unwrap();
        });
        cx.run_until_parked();
        cx.update(|cx| {
            let registry = ThemeRegistry::global(cx);
            assert!(registry.theme("Mine Dark").is_some());
            assert!(registry.theme("Bundled").is_some());
        });
    }
}
