//! Highlight queries compiled once per language and shared by every
//! highlighter of that language.
//!
//! Compiling a language's queries takes tens of milliseconds (about 19 ms for
//! Rust's highlights, 35 ms for TypeScript's), so building them again for
//! every editor, every code block and every injected layer was the larger
//! part of opening one. A compiled query is immutable, so one copy serves all
//! of them, on any thread.
//!
//! A language can also be compiled on a background thread, where its
//! grammar's tables are decoded too (another 20 to 600 ms the first time a
//! grammar is used). Code blocks ask for languages that way: until a language
//! is ready its blocks show plain text, and whoever asked is told when it is.

use std::{
    collections::HashMap,
    sync::{
        Arc, LazyLock, Mutex, RwLock,
        atomic::{AtomicU64, Ordering},
    },
};

use anyhow::{Context as _, Result};
use gpui::{BackgroundExecutor, SharedString};
use gpui_base::async_util::Sender;
use tree_sitter::Query;

use super::LanguageRegistry;

/// A highlight name (`keyword`, `string.escape`, …) interned for the
/// process, so cached highlights can name their capture in two bytes and be
/// resolved to a theme's colors later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct HighlightName(u16);

static NAMES: LazyLock<RwLock<(Vec<SharedString>, HashMap<SharedString, u16>)>> =
    LazyLock::new(Default::default);

impl HighlightName {
    fn intern(name: &str) -> Self {
        if let Some(id) = NAMES.read().unwrap().1.get(name) {
            return Self(*id);
        }
        let mut names = NAMES.write().unwrap();
        let (list, ids) = &mut *names;
        if let Some(id) = ids.get(name) {
            return Self(*id);
        }
        let id = u16::try_from(list.len()).expect("fewer than 65536 highlight names");
        let name = SharedString::from(name.to_string());
        list.push(name.clone());
        ids.insert(name, id);
        Self(id)
    }

    /// The name as the query wrote it.
    pub(crate) fn name(self) -> SharedString {
        NAMES.read().unwrap().0[self.0 as usize].clone()
    }
}

/// Whether the capture `name` is a highlight. Captures named `_…` only feed
/// a predicate of their pattern, and `spell` and `nospell` mark text for
/// spell checking, so a highlighter leaves them out.
pub(crate) fn is_highlight_capture(name: &str) -> bool {
    !name.starts_with('_') && name != "spell" && name != "nospell"
}

/// A query with the [`HighlightName`] of each of its highlight captures.
pub(crate) struct HighlightQuery {
    pub(crate) query: Query,
    names: Box<[Option<HighlightName>]>,
}

impl HighlightQuery {
    fn new(query: Query) -> Self {
        let names = query
            .capture_names()
            .iter()
            .map(|name| is_highlight_capture(name).then(|| HighlightName::intern(name)))
            .collect();
        Self { query, names }
    }

    /// The highlight name of the capture `index`, or `None` when it is not a
    /// highlight (see [`is_highlight_capture`]).
    pub(crate) fn name(&self, index: u32) -> Option<HighlightName> {
        self.names.get(index as usize).copied().flatten()
    }
}

/// A language's grammar with its queries compiled.
pub(crate) struct CompiledLanguage {
    pub(crate) name: SharedString,
    /// The source of [`Self::highlights`].
    pub(crate) highlights_source: SharedString,
    /// The injections, locals and highlights queries as one query, with the
    /// injection patterns disabled: injected layers are found separately.
    pub(crate) highlights: Arc<HighlightQuery>,
    pub(crate) locals_pattern_index: usize,
    pub(crate) highlights_pattern_index: usize,
    pub(crate) non_local_variable_patterns: Vec<bool>,
    pub(crate) local_scope_capture_index: Option<u32>,
    pub(crate) local_def_capture_index: Option<u32>,
    pub(crate) local_def_value_capture_index: Option<u32>,
    pub(crate) local_ref_capture_index: Option<u32>,
    pub(crate) injections: Option<Arc<Query>>,
    pub(crate) injection_content_capture_index: Option<u32>,
    pub(crate) injection_language_capture_index: Option<u32>,
}

/// A cache entry.
enum Slot<T> {
    /// Being compiled in the background, by a task of each of these
    /// [`Waiter`] domains.
    Compiling(Vec<u64>),
    /// Compiled, or `None` for a language without a grammar or whose queries
    /// failed to compile.
    Done(Option<T>),
}

#[derive(Default)]
struct Cache {
    /// The [`LanguageRegistry::generation`] of the last [`Self::sync`].
    generation: u64,
    /// By canonical language name.
    languages: HashMap<SharedString, Slot<Arc<CompiledLanguage>>>,
    /// The highlights query alone, for injected layers.
    layers: HashMap<SharedString, Slot<Arc<HighlightQuery>>>,
    /// Who to tell when the compilation of a name, by a task of a domain, is
    /// done. Kept when the entries are dropped: the task still runs and
    /// tells them, and they ask again.
    waiters: HashMap<(SharedString, u64), Vec<Sender<()>>>,
}

impl Cache {
    /// Drops the entries of the languages registered since the last sync.
    /// The others stay, so the code blocks of a language keep their
    /// highlighting while another one is registered.
    fn sync(&mut self) {
        let registry = LanguageRegistry::singleton();
        if self.generation == registry.generation() {
            return;
        }
        let (generation, registered) = registry.changes_since(self.generation);
        self.generation = generation;
        for name in &registered {
            self.languages.remove(name);
            self.layers.remove(name);
        }
    }
}

static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(Default::default);

/// Moves whenever a language finishes compiling in the background, so
/// highlights made while it was not ready can tell they are out of date.
static READY_GENERATION: AtomicU64 = AtomicU64::new(0);

/// See [`READY_GENERATION`].
pub(crate) fn ready_generation() -> u64 {
    READY_GENERATION.load(Ordering::Acquire)
}

/// Whether a language can be highlighted with now.
pub(crate) enum Readiness<T> {
    /// Compiled, or `None` when there is nothing to compile.
    Ready(Option<T>),
    /// Being compiled in the background.
    Pending,
}

/// Where a language that is not compiled yet gets compiled, and whom to
/// tell when it is ready.
///
/// Each waiter is a domain of its own, an application as a rule: its tasks
/// tell only its own senders, so no application is woken from another's
/// threads. A language another domain is compiling is compiled again for
/// this one.
#[derive(Clone)]
pub(crate) struct Waiter {
    pub(crate) domain: u64,
    pub(crate) executor: BackgroundExecutor,
    ready: Option<Sender<()>>,
}

impl Waiter {
    /// Compiles on `executor`'s threads and tells `ready`, if given.
    pub(crate) fn new(executor: BackgroundExecutor, ready: Option<Sender<()>>) -> Self {
        static NEXT_DOMAIN: AtomicU64 = AtomicU64::new(0);
        Self {
            domain: NEXT_DOMAIN.fetch_add(1, Ordering::Relaxed),
            executor,
            ready,
        }
    }
}

/// Stores what a background task of `domain`, started at the registry's
/// `generation`, compiled and tells the domain's waiters.
fn finish<T>(
    name: &SharedString,
    domain: u64,
    generation: u64,
    map: fn(&mut Cache) -> &mut HashMap<SharedString, Slot<T>>,
    compiled: Option<T>,
) {
    let waiters = {
        let mut cache = CACHE.lock().unwrap();
        cache.sync();
        if !LanguageRegistry::singleton().changed_since(name, generation)
            && let Some(slot @ Slot::Compiling(_)) = map(&mut cache).get_mut(name)
        {
            *slot = Slot::Done(compiled);
        }
        cache
            .waiters
            .remove(&(name.clone(), domain))
            .unwrap_or_default()
    };
    READY_GENERATION.fetch_add(1, Ordering::AcqRel);
    for waiter in waiters {
        _ = waiter.try_send(());
    }
}

/// Looks `name` up in the cache, or compiles it with `compile` on this
/// thread, outside the cache's lock, so compiling one language never blocks
/// a thread that wants another.
fn cached<T: Clone>(
    name: &SharedString,
    map: fn(&mut Cache) -> &mut HashMap<SharedString, Slot<T>>,
    compile: impl FnOnce() -> Option<T>,
) -> Option<T> {
    {
        let mut cache = CACHE.lock().unwrap();
        cache.sync();
        if let Some(Slot::Done(entry)) = map(&mut cache).get(name) {
            return entry.clone();
        }
    }

    let registry = LanguageRegistry::singleton();
    let generation = registry.generation();
    let compiled = compile();
    let mut cache = CACHE.lock().unwrap();
    cache.sync();
    if !registry.changed_since(name, generation) {
        // Tasks compiling it still tell their waiters when they are done.
        map(&mut cache).insert(name.clone(), Slot::Done(compiled.clone()));
    }
    compiled
}

/// Looks `name` up in the cache without compiling on this thread: a missing
/// entry is compiled by `compile` on `waiter`'s executor, which then tells
/// `waiter`.
fn cached_or_queued<T: Clone + Send + 'static>(
    name: &SharedString,
    map: fn(&mut Cache) -> &mut HashMap<SharedString, Slot<T>>,
    compile: fn(&SharedString) -> Option<T>,
    waiter: &Waiter,
) -> Readiness<T> {
    let mut cache = CACHE.lock().unwrap();
    cache.sync();
    let generation = cache.generation;
    let spawn = match map(&mut cache).get_mut(name) {
        Some(Slot::Done(entry)) => return Readiness::Ready(entry.clone()),
        Some(Slot::Compiling(domains)) => {
            let spawn = !domains.contains(&waiter.domain);
            if spawn {
                domains.push(waiter.domain);
            }
            spawn
        }
        None => {
            map(&mut cache).insert(name.clone(), Slot::Compiling(vec![waiter.domain]));
            true
        }
    };
    if let Some(ready) = &waiter.ready {
        cache
            .waiters
            .entry((name.clone(), waiter.domain))
            .or_default()
            .push(ready.clone());
    }
    drop(cache);

    if spawn {
        let name = name.clone();
        let domain = waiter.domain;
        waiter
            .executor
            .spawn(async move {
                let compiled = compile(&name);
                finish(&name, domain, generation, map, compiled);
            })
            .detach();
    }
    Readiness::Pending
}

/// The compiled queries of the language `name`, by its canonical name.
fn compile(name: &SharedString) -> Option<Arc<CompiledLanguage>> {
    if !LanguageRegistry::singleton().has_parser(name) {
        return None;
    }
    compile_language(name)
        .inspect_err(|err| tracing::error!("failed to compile the queries of {name:?}: {err:?}"))
        .ok()
        .map(Arc::new)
}

/// The highlights query of the language `name`, by its canonical name.
fn compile_layer(name: &SharedString) -> Option<Arc<HighlightQuery>> {
    let registry = LanguageRegistry::singleton();
    let config = registry.language(name)?;
    let grammar = registry.grammar(name).ok()?;
    Query::new(&grammar, &config.highlights)
        .inspect_err(|err| tracing::error!("failed to build injection query for {name:?}: {err:?}"))
        .ok()
        .map(|query| Arc::new(HighlightQuery::new(query)))
}

/// The compiled queries of the language `name` (or one of its aliases), or
/// `None` when it has no grammar. Compiles them on this thread when needed.
pub(crate) fn compiled_language(name: &str) -> Option<Arc<CompiledLanguage>> {
    let name = LanguageRegistry::singleton().canonical_name(name)?;
    cached(&name, |cache| &mut cache.languages, || compile(&name))
}

/// [`compiled_language`] without compiling on this thread: a language that
/// is not compiled yet is compiled in the background, and `waiter` is told
/// when it is ready.
pub(crate) fn compiled_language_now(
    name: &str,
    waiter: &Waiter,
) -> Readiness<Arc<CompiledLanguage>> {
    let Some(name) = LanguageRegistry::singleton().canonical_name(name) else {
        return Readiness::Ready(None);
    };
    cached_or_queued(&name, |cache| &mut cache.languages, compile, waiter)
}

/// The canonical name and the highlights query of the language `name`, for
/// a layer injected into another language. Compiles it on this thread when
/// needed.
pub(crate) fn layer_query(name: &str) -> Option<(SharedString, Arc<HighlightQuery>)> {
    let name = LanguageRegistry::singleton().canonical_name(name)?;
    let query = cached(&name, |cache| &mut cache.layers, || compile_layer(&name))?;
    Some((name, query))
}

/// [`layer_query`] without compiling on this thread, as
/// [`compiled_language_now`].
pub(crate) fn layer_query_now(
    name: &str,
    waiter: &Waiter,
) -> Readiness<(SharedString, Arc<HighlightQuery>)> {
    let Some(name) = LanguageRegistry::singleton().canonical_name(name) else {
        return Readiness::Ready(None);
    };
    match cached_or_queued(&name, |cache| &mut cache.layers, compile_layer, waiter) {
        Readiness::Ready(query) => Readiness::Ready(query.map(|query| (name, query))),
        Readiness::Pending => Readiness::Pending,
    }
}

/// Compiles `languages` in the background for `waiter`'s domain, unless
/// they are compiled already. The query of a layer one of them is injected
/// into another language with is compiled when such a layer first needs it.
pub(crate) fn warm_up(languages: impl IntoIterator<Item = impl AsRef<str>>, waiter: &Waiter) {
    for language in languages {
        _ = compiled_language_now(language.as_ref(), waiter);
    }
}

/// Compiles the queries of `name`, as tree-sitter-highlight does.
///
/// https://github.com/tree-sitter/tree-sitter/blob/v0.26.8/crates/highlight/src/highlight.rs#L339
fn compile_language(name: &str) -> Result<CompiledLanguage> {
    let registry = LanguageRegistry::singleton();
    let config = registry
        .language(name)
        .context("language is not registered")?;
    let grammar = registry.grammar(name)?;

    // Concatenate the query strings, keeping track of the start offset of each section.
    let mut query_source = String::new();
    query_source.push_str(&config.injections);
    let locals_query_offset = query_source.len();
    query_source.push_str(&config.locals);
    let highlights_query_offset = query_source.len();
    query_source.push_str(&config.highlights);

    // Construct a single query by concatenating the three query strings, but record the
    // range of pattern indices that belong to each individual string.
    let mut query = Query::new(&grammar, &query_source).context("new query")?;

    let mut locals_pattern_index = 0;
    let mut highlights_pattern_index = 0;
    for i in 0..(query.pattern_count()) {
        let pattern_offset = query.start_byte_for_pattern(i);
        if pattern_offset < highlights_query_offset {
            highlights_pattern_index += 1;
            if pattern_offset < locals_query_offset {
                locals_pattern_index += 1;
            }
        }
    }

    let injections = if !config.injections.is_empty() {
        Query::new(&grammar, &config.injections).ok().map(Arc::new)
    } else {
        None
    };

    // Injection layers are computed separately during parsing, so do not
    // emit injection captures from the main highlight query.
    for pattern_index in 0..locals_pattern_index {
        query.disable_pattern(pattern_index);
    }

    // Find all of the highlighting patterns that are disabled for nodes that
    // have been identified as local variables.
    let non_local_variable_patterns = (0..query.pattern_count())
        .map(|i| {
            query
                .property_predicates(i)
                .iter()
                .any(|(prop, positive)| !*positive && prop.key.as_ref() == "local")
        })
        .collect();

    let capture_index = |query: &Query, name: &str| {
        query
            .capture_names()
            .iter()
            .position(|capture| *capture == name)
            .map(|i| i as u32)
    };
    let injection_content_capture_index = injections
        .as_ref()
        .and_then(|q| capture_index(q, "injection.content"));
    let injection_language_capture_index = injections
        .as_ref()
        .and_then(|q| capture_index(q, "injection.language"));

    Ok(CompiledLanguage {
        name: config.name.clone(),
        highlights_source: query_source.into(),
        local_scope_capture_index: capture_index(&query, "local.scope"),
        local_def_capture_index: capture_index(&query, "local.definition"),
        local_def_value_capture_index: capture_index(&query, "local.definition-value"),
        local_ref_capture_index: capture_index(&query, "local.reference"),
        highlights: Arc::new(HighlightQuery::new(query)),
        locals_pattern_index,
        highlights_pattern_index,
        non_local_variable_patterns,
        injections,
        injection_content_capture_index,
        injection_language_capture_index,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A predicate a query doesn't know is kept as a general predicate,
    /// which the highlighter ignores, so its pattern matched everything the
    /// rest of it matched: `#lua-match?` and `#has-ancestor?` from
    /// nvim-treesitter, say.
    #[cfg(feature = "tree-sitter-languages")]
    #[test]
    fn bundled_queries_have_no_predicates_the_highlighter_ignores() {
        let mut ignored = Vec::new();
        for name in LanguageRegistry::singleton().languages() {
            let Some(language) = compiled_language(&name) else {
                continue;
            };
            let queries = [
                Some(&language.highlights.query),
                language.injections.as_deref(),
            ];
            for query in queries.into_iter().flatten() {
                for pattern in 0..query.pattern_count() {
                    for predicate in query.general_predicates(pattern) {
                        ignored.push(format!(
                            "{name}: #{} at byte {}",
                            predicate.operator,
                            query.start_byte_for_pattern(pattern)
                        ));
                    }
                }
            }
        }
        assert!(ignored.is_empty(), "{ignored:#?}");
    }

    #[test]
    fn a_language_is_compiled_once_and_shared() {
        let first = compiled_language("json").expect("json has a grammar");
        let again = compiled_language("jsonc").expect("an alias finds the same language");
        assert!(Arc::ptr_eq(&first, &again));
        assert_eq!(first.name.as_ref(), "json");
        assert!(
            compiled_language("text").is_none(),
            "plain text has no grammar"
        );
        assert!(compiled_language("no-such-language").is_none());
    }

    #[test]
    fn registering_a_language_recompiles_only_that_language() {
        use crate::highlighter::LanguageConfig;

        let register = |highlights: &str| {
            LanguageRegistry::singleton().register(
                "json-registered-twice",
                &LanguageConfig::new(
                    "json-registered-twice",
                    tree_sitter_json::LANGUAGE.into(),
                    vec![],
                    highlights,
                    "",
                    "",
                ),
            );
        };
        let json = compiled_language("json").expect("json has a grammar");
        register("(number) @number");
        let first = compiled_language("json-registered-twice").expect("a grammar");
        assert!(
            Arc::ptr_eq(&compiled_language("json").unwrap(), &json),
            "another language stays compiled"
        );

        register("(string) @string");
        let second = compiled_language("json-registered-twice").expect("a grammar");
        assert!(!Arc::ptr_eq(&first, &second));
        assert!(second.highlights_source.contains("@string"));
        assert!(Arc::ptr_eq(&compiled_language("json").unwrap(), &json));
    }

    #[test]
    fn highlight_names_are_interned() {
        let a = HighlightName::intern("keyword.test-intern");
        let b = HighlightName::intern("keyword.test-intern");
        assert_eq!(a, b);
        assert_eq!(a.name().as_ref(), "keyword.test-intern");
        assert_ne!(a, HighlightName::intern("string.test-intern"));
    }
}
