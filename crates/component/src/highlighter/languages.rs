use gpui::SharedString;

use crate::highlighter::GrammarConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, enum_iterator::Sequence)]
pub enum Language {
    Json,
    Plain,
    #[cfg(feature = "tree-sitter-astro")]
    Astro,
    #[cfg(feature = "tree-sitter-bash")]
    Bash,
    #[cfg(feature = "tree-sitter-c")]
    C,
    #[cfg(feature = "tree-sitter-cmake")]
    CMake,
    #[cfg(feature = "tree-sitter-csharp")]
    CSharp,
    #[cfg(feature = "tree-sitter-cpp")]
    Cpp,
    #[cfg(feature = "tree-sitter-css")]
    Css,
    #[cfg(feature = "tree-sitter-diff")]
    Diff,
    #[cfg(feature = "tree-sitter-ejs")]
    Ejs,
    #[cfg(feature = "tree-sitter-elixir")]
    Elixir,
    #[cfg(feature = "tree-sitter-erb")]
    Erb,
    #[cfg(feature = "tree-sitter-go")]
    Go,
    #[cfg(feature = "tree-sitter-graphql")]
    GraphQL,
    #[cfg(feature = "tree-sitter-html")]
    Html,
    #[cfg(feature = "tree-sitter-java")]
    Java,
    #[cfg(feature = "tree-sitter-javascript")]
    JavaScript,
    #[cfg(feature = "tree-sitter-jsdoc")]
    JsDoc,
    #[cfg(feature = "tree-sitter-kotlin")]
    Kotlin,
    #[cfg(feature = "tree-sitter-lua")]
    Lua,
    #[cfg(feature = "tree-sitter-make")]
    Make,
    #[cfg(feature = "tree-sitter-markdown")]
    Markdown,
    #[cfg(feature = "tree-sitter-markdown")]
    MarkdownInline,
    #[cfg(feature = "tree-sitter-php")]
    Php,
    #[cfg(feature = "tree-sitter-proto")]
    Proto,
    #[cfg(feature = "tree-sitter-python")]
    Python,
    #[cfg(feature = "tree-sitter-ruby")]
    Ruby,
    #[cfg(feature = "tree-sitter-rust")]
    Rust,
    #[cfg(feature = "tree-sitter-scala")]
    Scala,
    #[cfg(feature = "tree-sitter-sql")]
    Sql,
    #[cfg(feature = "tree-sitter-svelte")]
    Svelte,
    #[cfg(feature = "tree-sitter-swift")]
    Swift,
    #[cfg(feature = "tree-sitter-toml")]
    Toml,
    #[cfg(feature = "tree-sitter-tsx")]
    Tsx,
    #[cfg(feature = "tree-sitter-typescript")]
    TypeScript,
    #[cfg(feature = "tree-sitter-yaml")]
    Yaml,
    #[cfg(feature = "tree-sitter-zig")]
    Zig,
}

impl From<Language> for SharedString {
    fn from(language: Language) -> Self {
        language.name().into()
    }
}

impl Language {
    pub fn all() -> impl Iterator<Item = Self> {
        enum_iterator::all::<Language>()
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Plain => "text",
            #[cfg(feature = "tree-sitter-astro")]
            Self::Astro => "astro",
            #[cfg(feature = "tree-sitter-bash")]
            Self::Bash => "bash",
            #[cfg(feature = "tree-sitter-c")]
            Self::C => "c",
            #[cfg(feature = "tree-sitter-cmake")]
            Self::CMake => "cmake",
            #[cfg(feature = "tree-sitter-csharp")]
            Self::CSharp => "csharp",
            #[cfg(feature = "tree-sitter-cpp")]
            Self::Cpp => "cpp",
            #[cfg(feature = "tree-sitter-css")]
            Self::Css => "css",
            #[cfg(feature = "tree-sitter-diff")]
            Self::Diff => "diff",
            #[cfg(feature = "tree-sitter-ejs")]
            Self::Ejs => "ejs",
            #[cfg(feature = "tree-sitter-elixir")]
            Self::Elixir => "elixir",
            #[cfg(feature = "tree-sitter-erb")]
            Self::Erb => "erb",
            #[cfg(feature = "tree-sitter-go")]
            Self::Go => "go",
            #[cfg(feature = "tree-sitter-graphql")]
            Self::GraphQL => "graphql",
            #[cfg(feature = "tree-sitter-html")]
            Self::Html => "html",
            #[cfg(feature = "tree-sitter-java")]
            Self::Java => "java",
            #[cfg(feature = "tree-sitter-javascript")]
            Self::JavaScript => "javascript",
            #[cfg(feature = "tree-sitter-jsdoc")]
            Self::JsDoc => "jsdoc",
            #[cfg(feature = "tree-sitter-kotlin")]
            Self::Kotlin => "kotlin",
            #[cfg(feature = "tree-sitter-lua")]
            Self::Lua => "lua",
            #[cfg(feature = "tree-sitter-make")]
            Self::Make => "make",
            #[cfg(feature = "tree-sitter-markdown")]
            Self::Markdown => "markdown",
            #[cfg(feature = "tree-sitter-markdown")]
            Self::MarkdownInline => "markdown_inline",
            #[cfg(feature = "tree-sitter-php")]
            Self::Php => "php",
            #[cfg(feature = "tree-sitter-proto")]
            Self::Proto => "proto",
            #[cfg(feature = "tree-sitter-python")]
            Self::Python => "python",
            #[cfg(feature = "tree-sitter-ruby")]
            Self::Ruby => "ruby",
            #[cfg(feature = "tree-sitter-rust")]
            Self::Rust => "rust",
            #[cfg(feature = "tree-sitter-scala")]
            Self::Scala => "scala",
            #[cfg(feature = "tree-sitter-sql")]
            Self::Sql => "sql",
            #[cfg(feature = "tree-sitter-svelte")]
            Self::Svelte => "svelte",
            #[cfg(feature = "tree-sitter-swift")]
            Self::Swift => "swift",
            #[cfg(feature = "tree-sitter-toml")]
            Self::Toml => "toml",
            #[cfg(feature = "tree-sitter-tsx")]
            Self::Tsx => "tsx",
            #[cfg(feature = "tree-sitter-typescript")]
            Self::TypeScript => "typescript",
            #[cfg(feature = "tree-sitter-yaml")]
            Self::Yaml => "yaml",
            #[cfg(feature = "tree-sitter-zig")]
            Self::Zig => "zig",
        }
    }

    #[allow(unused)]
    pub fn from_str(s: &str) -> Self {
        Self::from_name(s).unwrap_or(Self::Plain)
    }

    pub(crate) fn from_name(s: &str) -> Option<Self> {
        match s {
            "text" | "plain" | "plaintext" => Some(Self::Plain),
            "json" | "jsonc" => Some(Self::Json),
            #[cfg(feature = "tree-sitter-astro")]
            "astro" => Some(Self::Astro),
            #[cfg(feature = "tree-sitter-bash")]
            "bash" | "sh" => Some(Self::Bash),
            #[cfg(feature = "tree-sitter-c")]
            "c" => Some(Self::C),
            #[cfg(feature = "tree-sitter-cmake")]
            "cmake" => Some(Self::CMake),
            #[cfg(feature = "tree-sitter-cpp")]
            "cpp" | "c++" => Some(Self::Cpp),
            #[cfg(feature = "tree-sitter-csharp")]
            "csharp" | "cs" => Some(Self::CSharp),
            #[cfg(feature = "tree-sitter-css")]
            "css" | "scss" => Some(Self::Css),
            #[cfg(feature = "tree-sitter-diff")]
            "diff" => Some(Self::Diff),
            #[cfg(feature = "tree-sitter-ejs")]
            "ejs" => Some(Self::Ejs),
            #[cfg(feature = "tree-sitter-elixir")]
            "elixir" | "ex" => Some(Self::Elixir),
            #[cfg(feature = "tree-sitter-erb")]
            "erb" => Some(Self::Erb),
            #[cfg(feature = "tree-sitter-go")]
            "go" => Some(Self::Go),
            #[cfg(feature = "tree-sitter-graphql")]
            "graphql" => Some(Self::GraphQL),
            #[cfg(feature = "tree-sitter-html")]
            "html" => Some(Self::Html),
            #[cfg(feature = "tree-sitter-java")]
            "java" => Some(Self::Java),
            #[cfg(feature = "tree-sitter-javascript")]
            "javascript" | "js" => Some(Self::JavaScript),
            #[cfg(feature = "tree-sitter-jsdoc")]
            "jsdoc" => Some(Self::JsDoc),
            #[cfg(feature = "tree-sitter-kotlin")]
            "kt" | "kts" | "ktm" | "kotlin" => Some(Self::Kotlin),
            #[cfg(feature = "tree-sitter-lua")]
            "lua" => Some(Self::Lua),
            #[cfg(feature = "tree-sitter-make")]
            "make" | "makefile" => Some(Self::Make),
            #[cfg(feature = "tree-sitter-markdown")]
            "markdown" | "md" | "mdx" => Some(Self::Markdown),
            #[cfg(feature = "tree-sitter-markdown")]
            "markdown_inline" | "markdown-inline" => Some(Self::MarkdownInline),
            #[cfg(feature = "tree-sitter-php")]
            "php" | "php3" | "php4" | "php5" | "phtml" => Some(Self::Php),
            #[cfg(feature = "tree-sitter-proto")]
            "proto" | "protobuf" => Some(Self::Proto),
            #[cfg(feature = "tree-sitter-python")]
            "python" | "py" => Some(Self::Python),
            #[cfg(feature = "tree-sitter-ruby")]
            "ruby" | "rb" => Some(Self::Ruby),
            #[cfg(feature = "tree-sitter-rust")]
            "rust" | "rs" => Some(Self::Rust),
            #[cfg(feature = "tree-sitter-scala")]
            "scala" => Some(Self::Scala),
            #[cfg(feature = "tree-sitter-sql")]
            "sql" => Some(Self::Sql),
            #[cfg(feature = "tree-sitter-svelte")]
            "svelte" => Some(Self::Svelte),
            #[cfg(feature = "tree-sitter-swift")]
            "swift" => Some(Self::Swift),
            #[cfg(feature = "tree-sitter-toml")]
            "toml" => Some(Self::Toml),
            #[cfg(feature = "tree-sitter-tsx")]
            "tsx" => Some(Self::Tsx),
            #[cfg(feature = "tree-sitter-typescript")]
            "typescript" | "ts" => Some(Self::TypeScript),
            #[cfg(feature = "tree-sitter-yaml")]
            "yaml" | "yml" => Some(Self::Yaml),
            #[cfg(feature = "tree-sitter-zig")]
            "zig" => Some(Self::Zig),
            _ => None,
        }
    }

    #[allow(unused)]
    pub(super) fn injection_languages(&self) -> Vec<SharedString> {
        let mut languages: Vec<&'static str> = Vec::new();

        match self {
            #[cfg(feature = "tree-sitter-markdown")]
            Self::Markdown => {
                languages.push("markdown_inline");
                #[cfg(feature = "tree-sitter-html")]
                languages.push("html");
                #[cfg(feature = "tree-sitter-toml")]
                languages.push("toml");
                #[cfg(feature = "tree-sitter-yaml")]
                languages.push("yaml");
            }
            #[cfg(feature = "tree-sitter-html")]
            Self::Html => {
                #[cfg(feature = "tree-sitter-javascript")]
                languages.push("javascript");
                #[cfg(feature = "tree-sitter-css")]
                languages.push("css");
            }
            #[cfg(feature = "tree-sitter-rust")]
            Self::Rust => {
                languages.push("rust");
            }
            #[cfg(feature = "tree-sitter-javascript")]
            Self::JavaScript => {
                #[cfg(feature = "tree-sitter-jsdoc")]
                languages.push("jsdoc");
                languages.push("json");
                #[cfg(feature = "tree-sitter-css")]
                languages.push("css");
                #[cfg(feature = "tree-sitter-html")]
                languages.push("html");
                #[cfg(feature = "tree-sitter-sql")]
                languages.push("sql");
                #[cfg(feature = "tree-sitter-typescript")]
                languages.push("typescript");
                languages.push("javascript");
                #[cfg(feature = "tree-sitter-tsx")]
                languages.push("tsx");
                #[cfg(feature = "tree-sitter-yaml")]
                languages.push("yaml");
                #[cfg(feature = "tree-sitter-graphql")]
                languages.push("graphql");
            }
            #[cfg(feature = "tree-sitter-typescript")]
            Self::TypeScript => {
                #[cfg(feature = "tree-sitter-jsdoc")]
                languages.push("jsdoc");
                languages.push("json");
                #[cfg(feature = "tree-sitter-css")]
                languages.push("css");
                #[cfg(feature = "tree-sitter-html")]
                languages.push("html");
                #[cfg(feature = "tree-sitter-sql")]
                languages.push("sql");
                languages.push("typescript");
                #[cfg(feature = "tree-sitter-javascript")]
                languages.push("javascript");
                #[cfg(feature = "tree-sitter-tsx")]
                languages.push("tsx");
                #[cfg(feature = "tree-sitter-yaml")]
                languages.push("yaml");
                #[cfg(feature = "tree-sitter-graphql")]
                languages.push("graphql");
            }
            #[cfg(feature = "tree-sitter-astro")]
            Self::Astro => {
                #[cfg(feature = "tree-sitter-html")]
                languages.push("html");
                #[cfg(feature = "tree-sitter-css")]
                languages.push("css");
                #[cfg(feature = "tree-sitter-javascript")]
                languages.push("javascript");
                #[cfg(feature = "tree-sitter-typescript")]
                languages.push("typescript");
            }
            #[cfg(feature = "tree-sitter-php")]
            Self::Php => {
                languages.push("php");
                #[cfg(feature = "tree-sitter-html")]
                languages.push("html");
                #[cfg(feature = "tree-sitter-css")]
                languages.push("css");
                #[cfg(feature = "tree-sitter-javascript")]
                languages.push("javascript");
                languages.push("json");
                #[cfg(feature = "tree-sitter-jsdoc")]
                languages.push("jsdoc");
                #[cfg(feature = "tree-sitter-graphql")]
                languages.push("graphql");
            }
            #[cfg(feature = "tree-sitter-svelte")]
            Self::Svelte => {
                languages.push("svelte");
                #[cfg(feature = "tree-sitter-html")]
                languages.push("html");
                #[cfg(feature = "tree-sitter-css")]
                languages.push("css");
                #[cfg(feature = "tree-sitter-typescript")]
                languages.push("typescript");
            }
            _ => {}
        }

        languages.into_iter().map(SharedString::from).collect()
    }

    /// Return the language info for the language.
    ///
    /// (language, query, injection, locals)
    pub(super) fn config(&self) -> GrammarConfig {
        let (language, query, injection, locals) = match self {
            Self::Plain => return GrammarConfig::plain(self.name()),
            Self::Json => (
                tree_sitter_json::LANGUAGE,
                include_str!("languages/json/highlights.scm"),
                "",
                "",
            ),
            #[cfg(feature = "tree-sitter-markdown")]
            Self::Markdown => (
                tree_sitter_md::LANGUAGE,
                include_str!("languages/markdown/highlights.scm"),
                include_str!("languages/markdown/injections.scm"),
                "",
            ),
            #[cfg(feature = "tree-sitter-markdown")]
            Self::MarkdownInline => (
                tree_sitter_md::INLINE_LANGUAGE,
                include_str!("languages/markdown_inline/highlights.scm"),
                "",
                "",
            ),
            #[cfg(feature = "tree-sitter-toml")]
            Self::Toml => (
                tree_sitter_toml_ng::LANGUAGE,
                include_str!("languages/toml/highlights.scm"),
                "",
                "",
            ),
            #[cfg(feature = "tree-sitter-yaml")]
            Self::Yaml => (
                tree_sitter_yaml::LANGUAGE,
                include_str!("languages/yaml/highlights.scm"),
                "",
                "",
            ),
            #[cfg(feature = "tree-sitter-rust")]
            Self::Rust => (
                tree_sitter_rust::LANGUAGE,
                include_str!("languages/rust/highlights.scm"),
                include_str!("languages/rust/injections.scm"),
                "",
            ),
            #[cfg(feature = "tree-sitter-go")]
            Self::Go => (
                tree_sitter_go::LANGUAGE,
                include_str!("languages/go/highlights.scm"),
                "",
                "",
            ),
            #[cfg(feature = "tree-sitter-c")]
            Self::C => (
                tree_sitter_c::LANGUAGE,
                include_str!("languages/c/highlights.scm"),
                "",
                "",
            ),
            #[cfg(feature = "tree-sitter-cpp")]
            Self::Cpp => (
                tree_sitter_cpp::LANGUAGE,
                include_str!("languages/cpp/highlights.scm"),
                "",
                "",
            ),
            #[cfg(feature = "tree-sitter-javascript")]
            Self::JavaScript => (
                tree_sitter_javascript::LANGUAGE,
                include_str!("languages/javascript/highlights.scm"),
                include_str!("languages/javascript/injections.scm"),
                include_str!("languages/javascript/locals.scm"),
            ),
            #[cfg(feature = "tree-sitter-jsdoc")]
            Self::JsDoc => (
                tree_sitter_jsdoc::LANGUAGE,
                include_str!("languages/jsdoc/highlights.scm"),
                "",
                "",
            ),
            #[cfg(feature = "tree-sitter-zig")]
            Self::Zig => (
                tree_sitter_zig::LANGUAGE,
                include_str!("languages/zig/highlights.scm"),
                include_str!("languages/zig/injections.scm"),
                "",
            ),
            #[cfg(feature = "tree-sitter-java")]
            Self::Java => (
                tree_sitter_java::LANGUAGE,
                include_str!("languages/java/highlights.scm"),
                "",
                "",
            ),
            #[cfg(feature = "tree-sitter-python")]
            Self::Python => (
                tree_sitter_python::LANGUAGE,
                include_str!("languages/python/highlights.scm"),
                "",
                "",
            ),
            #[cfg(feature = "tree-sitter-ruby")]
            Self::Ruby => (
                tree_sitter_ruby::LANGUAGE,
                include_str!("languages/ruby/highlights.scm"),
                "",
                include_str!("languages/ruby/locals.scm"),
            ),
            #[cfg(feature = "tree-sitter-bash")]
            Self::Bash => (
                tree_sitter_bash::LANGUAGE,
                include_str!("languages/bash/highlights.scm"),
                "",
                "",
            ),
            #[cfg(feature = "tree-sitter-html")]
            Self::Html => (
                tree_sitter_html::LANGUAGE,
                include_str!("languages/html/highlights.scm"),
                include_str!("languages/html/injections.scm"),
                "",
            ),
            #[cfg(feature = "tree-sitter-css")]
            Self::Css => (
                tree_sitter_css::LANGUAGE,
                include_str!("languages/css/highlights.scm"),
                "",
                "",
            ),
            #[cfg(feature = "tree-sitter-swift")]
            Self::Swift => (tree_sitter_swift::LANGUAGE, "", "", ""),
            #[cfg(feature = "tree-sitter-scala")]
            Self::Scala => (
                tree_sitter_scala::LANGUAGE,
                include_str!("languages/scala/highlights.scm"),
                "",
                include_str!("languages/scala/locals.scm"),
            ),
            #[cfg(feature = "tree-sitter-sql")]
            Self::Sql => (
                tree_sitter_sequel::LANGUAGE,
                include_str!("languages/sql/highlights.scm"),
                "",
                "",
            ),
            #[cfg(feature = "tree-sitter-csharp")]
            Self::CSharp => (
                tree_sitter_c_sharp::LANGUAGE,
                include_str!("languages/csharp/highlights.scm"),
                "",
                "",
            ),
            #[cfg(feature = "tree-sitter-graphql")]
            Self::GraphQL => (
                tree_sitter_graphql::LANGUAGE,
                include_str!("languages/graphql/highlights.scm"),
                "",
                "",
            ),
            #[cfg(feature = "tree-sitter-proto")]
            Self::Proto => (tree_sitter_proto::LANGUAGE, "", "", ""),
            #[cfg(feature = "tree-sitter-make")]
            Self::Make => (
                tree_sitter_make::LANGUAGE,
                include_str!("languages/make/highlights.scm"),
                "",
                "",
            ),
            #[cfg(feature = "tree-sitter-cmake")]
            Self::CMake => (tree_sitter_cmake::LANGUAGE, "", "", ""),
            #[cfg(feature = "tree-sitter-typescript")]
            Self::TypeScript => (
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
                include_str!("languages/typescript/highlights.scm"),
                include_str!("languages/javascript/injections.scm"),
                include_str!("languages/typescript/locals.scm"),
            ),
            #[cfg(feature = "tree-sitter-tsx")]
            Self::Tsx => (
                tree_sitter_typescript::LANGUAGE_TSX,
                include_str!("languages/tsx/highlights.scm"),
                "",
                include_str!("languages/typescript/locals.scm"),
            ),
            #[cfg(feature = "tree-sitter-diff")]
            Self::Diff => (
                tree_sitter_diff::LANGUAGE,
                include_str!("languages/diff/highlights.scm"),
                "",
                "",
            ),
            #[cfg(feature = "tree-sitter-elixir")]
            Self::Elixir => (
                tree_sitter_elixir::LANGUAGE,
                include_str!("languages/elixir/highlights.scm"),
                include_str!("languages/elixir/injections.scm"),
                "",
            ),
            #[cfg(feature = "tree-sitter-erb")]
            Self::Erb => (
                tree_sitter_embedded_template::LANGUAGE,
                include_str!("languages/embedded_template/highlights.scm"),
                include_str!("languages/embedded_template/injections-ejs.scm"),
                "",
            ),
            #[cfg(feature = "tree-sitter-ejs")]
            Self::Ejs => (
                tree_sitter_embedded_template::LANGUAGE,
                include_str!("languages/embedded_template/highlights.scm"),
                include_str!("languages/embedded_template/injections-ejs.scm"),
                "",
            ),
            #[cfg(feature = "tree-sitter-php")]
            Self::Php => (
                tree_sitter_php::LANGUAGE_PHP,
                include_str!("languages/php/highlights.scm"),
                include_str!("languages/php/injections.scm"),
                "",
            ),
            #[cfg(feature = "tree-sitter-astro")]
            Self::Astro => (
                tree_sitter_astro_next::LANGUAGE,
                include_str!("languages/astro/highlights.scm"),
                include_str!("languages/astro/injections.scm"),
                "",
            ),
            #[cfg(feature = "tree-sitter-kotlin")]
            Self::Kotlin => (
                tree_sitter_kotlin_ng::LANGUAGE,
                include_str!("languages/kotlin/highlights.scm"),
                "",
                "",
            ),
            #[cfg(feature = "tree-sitter-lua")]
            Self::Lua => (
                tree_sitter_lua::LANGUAGE,
                include_str!("languages/lua/highlights.scm"),
                include_str!("languages/lua/injections.scm"),
                include_str!("languages/lua/locals.scm"),
            ),
            #[cfg(feature = "tree-sitter-svelte")]
            Self::Svelte => (
                tree_sitter_svelte_ng::LANGUAGE,
                include_str!("languages/svelte/highlights.scm"),
                include_str!("languages/svelte/injections.scm"),
                "",
            ),
        };

        let language = tree_sitter::Language::new(language);

        GrammarConfig::new(
            self.name(),
            language,
            self.injection_languages(),
            query,
            injection,
            locals,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_name() {
        assert_eq!(Language::Plain.name(), "text");
        assert_eq!(Language::Json.name(), "json");

        #[cfg(feature = "tree-sitter-markdown")]
        {
            assert_eq!(Language::MarkdownInline.name(), "markdown_inline");
            assert_eq!(Language::Markdown.name(), "markdown");
        }

        #[cfg(feature = "tree-sitter-yaml")]
        assert_eq!(Language::Yaml.name(), "yaml");
        #[cfg(feature = "tree-sitter-rust")]
        assert_eq!(Language::Rust.name(), "rust");
        #[cfg(feature = "tree-sitter-go")]
        assert_eq!(Language::Go.name(), "go");
        #[cfg(feature = "tree-sitter-c")]
        assert_eq!(Language::C.name(), "c");
        #[cfg(feature = "tree-sitter-cpp")]
        assert_eq!(Language::Cpp.name(), "cpp");
        #[cfg(feature = "tree-sitter-sql")]
        assert_eq!(Language::Sql.name(), "sql");
        #[cfg(feature = "tree-sitter-javascript")]
        assert_eq!(Language::JavaScript.name(), "javascript");
        #[cfg(feature = "tree-sitter-zig")]
        assert_eq!(Language::Zig.name(), "zig");
        #[cfg(feature = "tree-sitter-csharp")]
        assert_eq!(Language::CSharp.name(), "csharp");
        #[cfg(feature = "tree-sitter-typescript")]
        assert_eq!(Language::TypeScript.name(), "typescript");
        #[cfg(feature = "tree-sitter-tsx")]
        assert_eq!(Language::Tsx.name(), "tsx");
        #[cfg(feature = "tree-sitter-diff")]
        assert_eq!(Language::Diff.name(), "diff");
        #[cfg(feature = "tree-sitter-elixir")]
        assert_eq!(Language::Elixir.name(), "elixir");
        #[cfg(feature = "tree-sitter-erb")]
        assert_eq!(Language::Erb.name(), "erb");
        #[cfg(feature = "tree-sitter-ejs")]
        assert_eq!(Language::Ejs.name(), "ejs");
    }

    #[test]
    fn test_language_aliases_only_resolve_enabled_features() {
        assert_eq!(Language::from_name("text"), Some(Language::Plain));
        assert_eq!(Language::from_name("jsonc"), Some(Language::Json));
        assert_eq!(Language::from_name("unknown"), None);

        #[cfg(feature = "tree-sitter-rust")]
        assert_eq!(Language::from_name("rs"), Some(Language::Rust));
        #[cfg(not(feature = "tree-sitter-rust"))]
        assert_eq!(Language::from_name("rs"), None);

        #[cfg(feature = "tree-sitter-markdown")]
        assert_eq!(Language::from_name("md"), Some(Language::Markdown));
        #[cfg(not(feature = "tree-sitter-markdown"))]
        assert_eq!(Language::from_name("md"), None);

        #[cfg(feature = "tree-sitter-typescript")]
        assert_eq!(Language::from_name("ts"), Some(Language::TypeScript));
        #[cfg(not(feature = "tree-sitter-typescript"))]
        assert_eq!(Language::from_name("ts"), None);

        assert_eq!(Language::from_str("unknown"), Language::Plain);
    }

    #[test]
    fn test_every_language_query_compiles() {
        let failures: Vec<String> = Language::all()
            .filter_map(|language| {
                let config = language.config();
                let grammar = config.language?;
                let source = format!(
                    "{}{}{}",
                    config.injections, config.locals, config.highlights
                );
                let err = tree_sitter::Query::new(&grammar, &source).err()?;
                Some(format!("{}: {err}", language.name()))
            })
            .collect();
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    #[cfg(feature = "tree-sitter-graphql")]
    fn test_graphql_highlights() {
        let source = r#"query Hero($episode: Episode = JEDI) @cached {
  hero(episode: $episode) {
    name
    ...HeroFields
  }
}

"The hero type."
type Character implements Node {
  friends(first: Int = 10): [Character!]!
}
"#;
        assert_highlights(
            "graphql",
            source,
            &[
                ("query", "keyword"),
                ("Hero", "function"),
                ("$episode", "variable.parameter"),
                ("Episode", "type"),
                ("JEDI", "constant"),
                ("cached", "attribute"),
                ("hero", "property"),
                ("HeroFields", "function"),
                ("\"The hero type.\"", "comment.doc"),
                ("implements", "keyword"),
                ("Character", "type"),
                ("friends", "property"),
                ("first", "variable.parameter"),
                ("10", "number"),
            ],
        );
    }

    #[test]
    #[cfg(feature = "tree-sitter-csharp")]
    fn test_csharp_highlights() {
        let source = r#"/// <summary>Greets people.</summary>
[Serializable]
public class Greeter<T> : IGreeter where T : class
{
    private const int MaxCount = 3;
    public string Name { get; init; } = "world";

    public async Task<string> GreetAsync(T target, int count = 1)
    {
        var message = $"Hello, {Name}!\n";
        await Task.Delay(count);
        return message ?? null;
    }
}
"#;
        assert_highlights(
            "csharp",
            source,
            &[
                ("/// <summary>Greets people.</summary>", "comment.doc"),
                ("Serializable", "attribute"),
                ("public", "keyword"),
                ("Greeter", "type"),
                ("IGreeter", "type"),
                ("MaxCount", "constant"),
                ("3", "number"),
                ("Name", "property"),
                ("\"world\"", "string"),
                ("GreetAsync", "function"),
                ("target", "variable.parameter"),
                ("int", "type.builtin"),
                ("Hello, ", "string"),
                ("\\n", "string.escape"),
                ("Delay", "function"),
                ("null", "constant.builtin"),
            ],
        );
    }

    /// Highlights `source` and asserts that each `(text, capture)` pair renders
    /// with that capture, using a theme that styles every highlight name.
    #[cfg(any(
        feature = "tree-sitter-cmake",
        feature = "tree-sitter-csharp",
        feature = "tree-sitter-graphql",
        feature = "tree-sitter-proto",
        feature = "tree-sitter-swift",
    ))]
    #[track_caller]
    fn assert_highlights(language: &str, source: &str, expected: &[(&str, &str)]) {
        use crate::highlighter::{SyntaxHighlighter, registry::HIGHLIGHT_NAMES};
        use std::sync::Mutex;

        /// Gives each capture name its own hue, so the rendered style tells which
        /// capture won. Names without a theme key stay unstyled, as in a real theme.
        #[derive(Default)]
        struct CaptureColors(Mutex<Vec<String>>);

        impl gpui_base::input::HighlightStyleResolver for CaptureColors {
            fn style(&self, name: &str) -> Option<gpui::HighlightStyle> {
                let prefix = name.split('.').next().unwrap_or(name);
                if !HIGHLIGHT_NAMES.contains(&name) && !HIGHLIGHT_NAMES.contains(&prefix) {
                    return None;
                }
                let mut names = self.0.lock().unwrap();
                let index = match names.iter().position(|n| n == name) {
                    Some(index) => index,
                    None => {
                        names.push(name.to_string());
                        names.len() - 1
                    }
                };
                Some(gpui::HighlightStyle {
                    color: Some(gpui::hsla(index as f32 / 1000., 1., 0.5, 1.)),
                    ..Default::default()
                })
            }
        }

        let rope = ropey::Rope::from_str(source);
        let mut highlighter = SyntaxHighlighter::new(language);
        highlighter.update(None, &rope, None);
        let colors = CaptureColors::default();
        let styles = highlighter.styles(&(0..source.len()), &colors);
        let names = colors.0.into_inner().unwrap();

        for (text, capture) in expected {
            let start = source
                .find(text)
                .unwrap_or_else(|| panic!("{text:?} is not in the {language} source"));
            let range = start..start + text.len();
            let rendered: Vec<&str> = styles
                .iter()
                .filter(|(style_range, _)| {
                    style_range.start < range.end && range.start < style_range.end
                })
                .map(|(_, style)| match style.color {
                    Some(color) => names[(color.h * 1000.).round() as usize].as_str(),
                    None => "(none)",
                })
                .collect();
            assert!(
                !rendered.is_empty() && rendered.iter().all(|name| name == capture),
                "{language}: {text:?} renders as {rendered:?}, expected {capture:?}"
            );
        }
    }
}
