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

        // A theme of the same name but the other mode (a user's copy that
        // changed it) doesn't take the slot: applying it would switch the
        // app's mode and overwrite the other slot.
        if let Some(theme) = ThemeRegistry::global(cx)
            .theme(&light_theme)
            .filter(|theme| !theme.mode.is_dark())
            .cloned()
        {
            Theme::global_mut(cx).light_theme = theme;
        }
        if let Some(theme) = ThemeRegistry::global(cx)
            .theme(&dark_theme)
            .filter(|theme| theme.mode.is_dark())
            .cloned()
        {
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
/// Themes come from four places, later ones replacing earlier ones of the
/// same name, compared ignoring case:
///
/// 1. the kit's two defaults;
/// 2. themes added with [`ThemeRegistry::load_themes_from_str`], which only
///    fill names nothing else takes;
/// 3. themes the application embeds in its binary, added with
///    [`ThemeRegistry::add_bundled_themes`];
/// 4. the folder [`ThemeRegistry::watch_dir`] watches (or
///    [`ThemeRegistry::load_dir`] read), the user's own themes.
///
/// A name of one of the kit's defaults is never taken by another theme. A
/// folder theme marked `is_default` becomes the default of its mode instead,
/// for as long as it stays in the folder.
///
/// The folder is read again whenever a file in it changes; the other themes
/// stay registered across those reloads.
#[derive(Default, Debug)]
pub struct ThemeRegistry {
    themes_dir: PathBuf,
    /// The kit's two defaults, as shipped.
    kit_default_themes: HashMap<ThemeMode, Rc<ThemeConfig>>,
    /// The defaults in use: the kit's, or folder themes marked default.
    default_themes: HashMap<ThemeMode, Rc<ThemeConfig>>,
    /// Themes added from strings, filling names nothing else takes.
    loaded_themes: Vec<Rc<ThemeConfig>>,
    /// Themes embedded in the application, in the order they were added.
    bundled_themes: Vec<Rc<ThemeConfig>>,
    /// The themes read from `themes_dir` at its last load.
    folder_themes: Vec<Rc<ThemeConfig>>,
    themes: HashMap<SharedString, Rc<ThemeConfig>>,
    has_custom_themes: bool,
    /// Advances with each [`ThemeRegistry::watch_dir`] or
    /// [`ThemeRegistry::load_dir`]: a read lands only if no later call
    /// asked for a folder since.
    folder_generation: u64,
    /// The task that reloads the watched folder; replacing it stops the
    /// previous watcher.
    folder_watch: Option<FolderWatch>,
}

/// The watched folder's reload task, which also owns its file watcher.
struct FolderWatch(#[allow(dead_code)] gpui::Task<()>);

impl std::fmt::Debug for FolderWatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FolderWatch")
    }
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
    /// read. The folder is created if it is missing. Watching another folder
    /// stops watching this one, and a read of an earlier folder that ends
    /// late is dropped. In tests, use [`ThemeRegistry::load_dir`]: the
    /// watcher runs on a thread of its own, which a `#[gpui::test]`'s
    /// deterministic executor rejects.
    #[cfg(not(target_family = "wasm"))]
    pub fn watch_dir<F>(themes_dir: PathBuf, cx: &mut App, on_load: F) -> Result<()>
    where
        F: FnOnce(&mut App) + 'static,
    {
        use notify::Watcher as _;

        let (tx, rx) = smol::channel::bounded(1);
        // Watch before the first read, so a change during it is seen.
        let watcher = (|| -> anyhow::Result<notify::RecommendedWatcher> {
            if !themes_dir.exists() {
                std::fs::create_dir_all(&themes_dir)?;
            }
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
            watcher.watch(&themes_dir, notify::RecursiveMode::NonRecursive)?;
            Ok(watcher)
        })();
        let watcher = match watcher {
            Ok(watcher) => Some(watcher),
            Err(err) => {
                tracing::error!("Failed to watch themes directory: {err}");
                None
            }
        };
        Self::watch_with(themes_dir, rx, watcher, cx, on_load);
        Ok(())
    }

    /// Load `themes_dir` now and again at each message on `changes`,
    /// keeping `watcher` alive while it does: [`ThemeRegistry::watch_dir`]
    /// without the file watcher, so tests can drive the reloads.
    #[cfg(not(target_family = "wasm"))]
    fn watch_with(
        themes_dir: PathBuf,
        changes: smol::channel::Receiver<()>,
        watcher: Option<impl std::any::Any + 'static>,
        cx: &mut App,
        on_load: impl FnOnce(&mut App) + 'static,
    ) {
        let generation = Self::global_mut(cx).start_folder(themes_dir.clone());
        let task = cx.spawn(async move |cx| {
            let _watcher = watcher;
            Self::load_folder(&themes_dir, generation, cx).await;
            _ = cx.update(on_load);
            while changes.recv().await.is_ok() {
                tracing::info!("Reloading themes...");
                Self::load_folder(&themes_dir, generation, cx).await;
            }
        });
        Self::global_mut(cx).folder_watch = Some(FolderWatch(task));
    }

    /// Read a folder of theme files once, without watching it, on a
    /// background thread, then run `on_load`. The themes replace bundled
    /// ones as [`ThemeRegistry::watch_dir`]'s do, and it stops a watch
    /// started before; a read of an earlier folder that ends late is
    /// dropped.
    ///
    /// Use it where hot reload isn't wanted, and in tests: the watcher
    /// `watch_dir` starts runs on a thread of its own, which a
    /// `#[gpui::test]`'s deterministic executor rejects.
    #[cfg(not(target_family = "wasm"))]
    pub fn load_dir<F>(themes_dir: PathBuf, cx: &mut App, on_load: F)
    where
        F: FnOnce(&mut App) + 'static,
    {
        let generation = Self::global_mut(cx).start_folder(themes_dir.clone());
        cx.spawn(async move |cx| {
            if Self::load_folder(&themes_dir, generation, cx).await {
                _ = cx.update(on_load);
            }
        })
        .detach();
    }

    /// Make `themes_dir` the folder to load, stopping any watch of an
    /// earlier one, and return the generation its reads carry.
    #[cfg(not(target_family = "wasm"))]
    fn start_folder(&mut self, themes_dir: PathBuf) -> u64 {
        self.themes_dir = themes_dir;
        self.folder_watch = None;
        self.folder_generation += 1;
        self.folder_generation
    }

    /// Read `themes_dir` on a background thread and register its themes,
    /// unless a later folder was asked for meanwhile. Returns whether they
    /// were registered.
    #[cfg(not(target_family = "wasm"))]
    async fn load_folder(
        themes_dir: &std::path::Path,
        generation: u64,
        cx: &mut gpui::AsyncApp,
    ) -> bool {
        let dir = themes_dir.to_path_buf();
        let themes = cx
            .background_executor()
            .spawn(async move { read_themes_dir(&dir) })
            .await;
        cx.update(|cx| {
            let registry = Self::global(cx);
            if registry.folder_generation != generation {
                return false;
            }
            Self::global_mut(cx).set_folder_themes(themes);
            true
        })
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

    /// Parse a theme file and register its themes under the names no other
    /// theme takes. They stay registered, and give way to a bundled or
    /// folder theme of the same name.
    pub fn load_themes_from_str(&mut self, content: &str) -> anyhow::Result<()> {
        let theme_set = serde_json::from_str::<ThemeSet>(content)?;
        for theme in theme_set.themes {
            self.loaded_themes
                .retain(|loaded| !same_name(&loaded.name, &theme.name));
            self.loaded_themes.push(Rc::new(theme));
        }
        self.rebuild();
        Ok(())
    }

    fn init_default_themes(&mut self) {
        let default_themes: Vec<ThemeConfig> = serde_json::from_str::<ThemeSet>(DEFAULT_THEME)
            .expect("failed to parse default theme.")
            .themes;
        for theme in default_themes.into_iter() {
            let mode = if theme.mode.is_dark() {
                ThemeMode::Dark
            } else {
                ThemeMode::Light
            };
            self.kit_default_themes.insert(mode, Rc::new(theme));
        }
        self.default_themes = self.kit_default_themes.clone();
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

    /// Replace the themes read from the watched folder.
    #[cfg(not(target_family = "wasm"))]
    fn set_folder_themes(&mut self, themes: Vec<ThemeConfig>) {
        self.folder_themes = themes.into_iter().map(Rc::new).collect();
        self.rebuild();
    }

    /// Compose the registered themes from the defaults, the bundled themes
    /// and the folder's themes, in that order of precedence from lowest,
    /// then fill free names with the themes loaded from strings.
    fn rebuild(&mut self) {
        // A folder theme marked default replaces the kit's default for its
        // mode while it stays in the folder.
        self.default_themes = self.kit_default_themes.clone();
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
        for theme in &self.loaded_themes {
            if !self.themes.keys().any(|name| same_name(name, &theme.name)) {
                self.themes.insert(theme.name.clone(), theme.clone());
            }
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
    fn loading_a_folder_once_adds_its_themes(cx: &mut TestAppContext) {
        let dir = ThemesDir::new("load");
        dir.write("mine.json", &theme_set(&[("Bundled", "light", "#123456")]));
        let loaded = std::rc::Rc::new(std::cell::Cell::new(false));
        cx.update(|cx| {
            crate::theme::init(cx);
            ThemeRegistry::global_mut(cx)
                .add_bundled_themes_from_str(&theme_set(&[("Bundled", "light", "#eeeeee")]))
                .unwrap();
            let loaded = loaded.clone();
            ThemeRegistry::load_dir(dir.0.clone(), cx, move |_| loaded.set(true));
        });
        cx.run_until_parked();
        assert!(loaded.get());
        cx.update(|cx| {
            assert_eq!(
                background(ThemeRegistry::global(cx), "Bundled").as_deref(),
                Some("#123456")
            );
        });
    }

    /// Two loads in a row: whichever read ends last, the folder asked for
    /// last is the one registered, and only its callback runs.
    #[gpui::test(iterations = 40)]
    fn the_folder_asked_for_last_wins(cx: &mut TestAppContext) {
        let first = ThemesDir::new("first");
        first.write("t.json", &theme_set(&[("Mine", "light", "#aaaaaa")]));
        let second = ThemesDir::new("second");
        second.write("t.json", &theme_set(&[("Mine", "light", "#bbbbbb")]));
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        cx.update(|cx| {
            crate::theme::init(cx);
            for (dir, name) in [(&first, "first"), (&second, "second")] {
                let calls = calls.clone();
                ThemeRegistry::load_dir(dir.0.clone(), cx, move |_| calls.borrow_mut().push(name));
            }
        });
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(
                background(ThemeRegistry::global(cx), "Mine").as_deref(),
                Some("#bbbbbb")
            );
        });
        assert_eq!(*calls.borrow(), vec!["second"]);
    }

    /// The watched folder reloads at each change, and watching another
    /// folder stops the first watch: its changes no longer land. Driven
    /// through the change channel `watch_dir` feeds from its file watcher.
    #[gpui::test]
    fn a_watched_folder_reloads_until_another_is_watched(cx: &mut TestAppContext) {
        let first = ThemesDir::new("watch-first");
        first.write("t.json", &theme_set(&[("Mine", "dark", "#111111")]));
        let second = ThemesDir::new("watch-second");
        second.write("t.json", &theme_set(&[("Other", "dark", "#999999")]));
        let (changed, changes) = smol::channel::bounded(1);
        let loaded = std::rc::Rc::new(std::cell::Cell::new(0));
        cx.update(|cx| {
            crate::theme::init(cx);
            let loaded = loaded.clone();
            ThemeRegistry::watch_with(first.0.clone(), changes, None::<()>, cx, move |_| {
                loaded.set(loaded.get() + 1)
            });
        });
        cx.run_until_parked();
        let mine =
            |cx: &mut TestAppContext| cx.update(|cx| background(ThemeRegistry::global(cx), "Mine"));
        assert_eq!(mine(cx).as_deref(), Some("#111111"));
        assert_eq!(loaded.get(), 1);

        first.write("t.json", &theme_set(&[("Mine", "dark", "#222222")]));
        changed.try_send(()).unwrap();
        cx.run_until_parked();
        assert_eq!(mine(cx).as_deref(), Some("#222222"));
        assert_eq!(
            loaded.get(),
            1,
            "the callback runs after the first read only"
        );

        let (_other_changed, other_changes) = smol::channel::bounded(1);
        cx.update(|cx| {
            ThemeRegistry::watch_with(second.0.clone(), other_changes, None::<()>, cx, |_| {})
        });
        cx.run_until_parked();
        first.write("t.json", &theme_set(&[("Mine", "dark", "#333333")]));
        assert!(
            changed.try_send(()).is_err(),
            "the first watch stopped with its task"
        );
        cx.run_until_parked();
        assert_eq!(mine(cx), None);
        cx.update(|cx| {
            assert_eq!(
                background(ThemeRegistry::global(cx), "Other").as_deref(),
                Some("#999999")
            )
        });
    }

    #[test]
    fn themes_from_strings_stay_and_give_way() {
        let mut registry = ThemeRegistry::default();
        registry.init_default_themes();
        registry
            .load_themes_from_str(&theme_set(&[
                ("Loaded", "light", "#010101"),
                ("Shared", "light", "#020202"),
            ]))
            .unwrap();
        registry
            .add_bundled_themes_from_str(&theme_set(&[("shared", "light", "#030303")]))
            .unwrap();
        registry.set_folder_themes(Vec::new());
        assert_eq!(background(&registry, "Loaded").as_deref(), Some("#010101"));
        assert_eq!(background(&registry, "Shared").as_deref(), Some("#030303"));
    }

    /// A folder theme marked default is the default while it is in the
    /// folder; once its file goes, the kit's default is back.
    #[test]
    fn a_folder_default_lasts_as_long_as_its_file() {
        let mut registry = ThemeRegistry::default();
        registry.init_default_themes();
        let mut mine: ThemeConfig = serde_json::from_value(serde_json::json!({
            "name": "My Default", "mode": "light", "colors": {}
        }))
        .unwrap();
        mine.is_default = true;
        registry.set_folder_themes(vec![mine]);
        assert_eq!(registry.default_light_theme().name.as_ref(), "My Default");
        assert!(registry.theme("Default Light").is_none());

        registry.set_folder_themes(Vec::new());
        assert_eq!(
            registry.default_light_theme().name.as_ref(),
            "Default Light"
        );
        assert!(registry.theme("My Default").is_none());
    }

    /// A user's copy of the active light theme that turned it dark doesn't
    /// take the light slot: the app stays in light mode on its theme.
    #[gpui::test]
    fn a_copy_in_the_other_mode_keeps_out_of_the_slot(cx: &mut TestAppContext) {
        let dir = ThemesDir::new("mode");
        dir.write("t.json", &theme_set(&[("classic", "dark", "#000000")]));
        cx.update(|cx| {
            crate::theme::init(cx);
            ThemeRegistry::global_mut(cx)
                .add_bundled_themes_from_str(&theme_set(&[("Classic", "light", "#fefefe")]))
                .unwrap();
            let classic = ThemeRegistry::global(cx).theme("Classic").cloned().unwrap();
            Theme::update(cx, |theme| theme.apply_config(&classic));
            ThemeRegistry::load_dir(dir.0.clone(), cx, |_| {});
        });
        cx.run_until_parked();
        cx.update(|cx| {
            let theme = Theme::global(cx);
            assert!(!theme.is_dark());
            assert_eq!(theme.light_theme.name.as_ref(), "Classic");
            assert!(!theme.light_theme.mode.is_dark());
        });
    }
}
