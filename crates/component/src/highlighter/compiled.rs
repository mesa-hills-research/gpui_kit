//! Highlight queries compiled once per language and shared by every
//! highlighter of that language.
//!
//! Compiling a language's queries takes tens of milliseconds (about 19 ms for
//! Rust's highlights, 35 ms for TypeScript's), so building them again for
//! every editor, every code block and every injected layer was the larger
//! part of opening one. A compiled query is immutable, so one copy serves all
//! of them, on any thread.

use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
};

use anyhow::{Context as _, Result};
use gpui::SharedString;
use tree_sitter::Query;

use super::LanguageRegistry;

/// A compiled highlights query.
pub(crate) struct HighlightQuery {
    pub(crate) query: Query,
}

impl HighlightQuery {
    fn new(query: Query) -> Self {
        Self { query }
    }
}

/// A language's grammar with its queries compiled.
pub(crate) struct CompiledLanguage {
    pub(crate) name: SharedString,
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

#[derive(Default)]
struct Cache {
    /// The [`LanguageRegistry::generation`] the entries were compiled at.
    generation: u64,
    /// By canonical language name; `None` for a language without a grammar
    /// or whose queries failed to compile.
    languages: HashMap<SharedString, Option<Arc<CompiledLanguage>>>,
    /// The highlights query alone, for injected layers.
    layers: HashMap<SharedString, Option<Arc<HighlightQuery>>>,
}

static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(Default::default);

/// Looks `name` up in the cache, or compiles it with `compile` outside the
/// lock, so compiling one language never blocks a thread that wants another.
fn cached<T: Clone>(
    name: &SharedString,
    map: impl Fn(&mut Cache) -> &mut HashMap<SharedString, Option<T>>,
    compile: impl FnOnce() -> Option<T>,
) -> Option<T> {
    let generation = LanguageRegistry::singleton().generation();
    {
        let mut cache = CACHE.lock().unwrap();
        if cache.generation != generation {
            *cache = Cache {
                generation,
                ..Default::default()
            };
        }
        if let Some(entry) = map(&mut cache).get(name) {
            return entry.clone();
        }
    }

    let compiled = compile();
    let mut cache = CACHE.lock().unwrap();
    if cache.generation == generation {
        map(&mut cache)
            .entry(name.clone())
            .or_insert_with(|| compiled.clone());
    }
    compiled
}

/// The compiled queries of the language `name` (or one of its aliases), or
/// `None` when it has no grammar.
pub(crate) fn compiled_language(name: &str) -> Option<Arc<CompiledLanguage>> {
    let registry = LanguageRegistry::singleton();
    let config = registry.language(name)?;
    cached(
        &config.name,
        |cache| &mut cache.languages,
        || {
            if !registry.has_parser(name) {
                return None;
            }
            compile_language(name)
                .inspect_err(|err| {
                    tracing::error!(
                        "failed to compile the queries of {:?}: {err:?}",
                        config.name
                    )
                })
                .ok()
                .map(Arc::new)
        },
    )
}

/// The canonical name and the highlights query of the language `name`, for
/// a layer injected into another language.
pub(crate) fn layer_query(name: &str) -> Option<(SharedString, Arc<HighlightQuery>)> {
    let registry = LanguageRegistry::singleton();
    let config = registry.language(name)?;
    let query = cached(
        &config.name,
        |cache| &mut cache.layers,
        || {
            let grammar = registry.grammar(name).ok()?;
            Query::new(&grammar, &config.highlights)
                .inspect_err(|err| {
                    tracing::error!(
                        "failed to build injection query for {:?}: {err:?}",
                        config.name
                    )
                })
                .ok()
                .map(|query| Arc::new(HighlightQuery::new(query)))
        },
    )?;
    Some((config.name, query))
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

    #[test]
    fn a_language_is_compiled_once_and_shared() {
        let generation = LanguageRegistry::singleton().generation();
        let first = compiled_language("json").expect("json has a grammar");
        let again = compiled_language("jsonc").expect("an alias finds the same language");
        // Tests running in parallel can register languages, which drops the
        // compiled ones. When none did, both lookups share one compilation.
        if LanguageRegistry::singleton().generation() == generation {
            assert!(Arc::ptr_eq(&first, &again));
        }
        assert_eq!(first.name.as_ref(), "json");
        assert!(
            compiled_language("text").is_none(),
            "plain text has no grammar"
        );
        assert!(compiled_language("no-such-language").is_none());
    }
}
