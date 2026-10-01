//! The index's fixed values in one place.

use std::time::Duration;

use crate::languages::{Container, LanguageSpec};

/// What becomes a searchable code unit and how it is stored.
pub struct IndexLimits;

impl IndexLimits {
    /// Characters of source the judge reads: the limit s1-code was trained with, so it changes only with the model.
    pub const JUDGE_SOURCE_CHARACTERS: usize = 1500;
    /// Shorter definitions (one-line getters, `pass` stubs) are not worth a search result.
    pub const MINIMUM_LINES: usize = 3;
    /// The outline of a unit, embedded first so a large project is searchable in a minute: its first lines (the
    /// signature and the start of the docstring), at most this many characters.
    pub const OUTLINE_LINES: usize = 4;
    pub const OUTLINE_CHARACTERS: usize = 320;
    /// Versions of the texts the retriever embeds (`CodeUnit::document_text` and `outline_text`, with the outline
    /// limits above). Change one whenever its text changes: vectors of the old text then stop being used.
    pub const WHOLE_TEXT_FORMAT: &'static str = "whole-v1";
    pub const OUTLINE_TEXT_FORMAT: &'static str = "outline-v2";
    /// Characters of a model revision kept in an embedding space key.
    pub const SPACE_REVISION_LENGTH: usize = 12;
    /// Hex characters kept from the blake3 hash that identifies a unit's content.
    pub const CONTENT_KEY_LENGTH: usize = 32;
    /// How long a write waits for another process holding the index database.
    pub const BUSY_TIMEOUT: Duration = Duration::from_secs(30);
}

/// The word index each project keeps beside its vectors (ADR-0007).
pub struct LexicalSettings;

impl LexicalSettings {
    /// Folder of the index inside the project's folder, and the version of its fields and tokenizer: an index of
    /// another version is rebuilt.
    pub const FOLDER: &'static str = "lexical";
    pub const FORMAT: &'static str = "lexical-v1";
    pub const FORMAT_FILE: &'static str = "format";
    /// Memory the index writer may use before it flushes (tantivy's minimum is 15 MB).
    pub const WRITER_MEMORY: usize = 50_000_000;
    /// How much a word counts in a function's name, its path and its code.
    pub const NAME_BOOST: f32 = 3.0;
    pub const PATH_BOOST: f32 = 1.5;
    pub const BODY_BOOST: f32 = 1.0;
    /// Shorter words are left out of queries.
    pub const MINIMUM_TERM_LENGTH: usize = 2;
    /// Words a query needs to be looked up as an exact phrase.
    pub const MINIMUM_PHRASE_WORDS: usize = 2;
    /// Hits fetched before keeping those in the searched folder.
    pub const OVERFETCH: usize = 8;
}

/// Fusion of the retriever's order with the judge's: (judge share, rank smoothing), chosen on the dev split of the exam
/// for s1-code v3, per retriever and number of candidates judged.
pub struct FusionTable;

impl FusionTable {
    pub const QWEN3_UP_TO_FIVE: (f64, f64) = (0.45, 5.0);
    pub const QWEN3_MORE: (f64, f64) = (0.35, 30.0);
    pub const UP_TO_FIVE: (f64, f64) = (0.55, 5.0);
    pub const UP_TO_TEN: (f64, f64) = (0.65, 1.0);
    pub const MORE: (f64, f64) = (0.75, 10.0);
}

/// The languages s1grep reads. Python has its own extractor, the one its training data and exams were built with;
/// the others are read through a tree-sitter query each. A language's version goes up whenever its query or rules
/// change, so the files read with the older version are read again.
pub struct LanguageSettings;

impl LanguageSettings {
    pub const PYTHON: &'static str = "python";
    pub const PYTHON_VERSION: &'static str = "python-1";
    pub const PYTHON_EXTENSIONS: &'static [&'static str] = &["py"];

    const JAVASCRIPT_CONTAINERS: &'static [Container] = &[
        Container {
            kind: "class_declaration",
            field: "name",
        },
        Container {
            kind: "class",
            field: "name",
        },
        Container {
            kind: "abstract_class_declaration",
            field: "name",
        },
    ];
    const JAVASCRIPT_FUNCTIONS: &'static [&'static str] = &[
        "function_declaration",
        "generator_function_declaration",
        "function_expression",
        "arrow_function",
        "method_definition",
        "function",
    ];

    pub fn specs() -> Vec<LanguageSpec> {
        vec![
            LanguageSpec {
                name: "javascript",
                version: 1,
                extensions: &["js", "jsx", "mjs", "cjs"],
                grammar: || tree_sitter_javascript::LANGUAGE.into(),
                query: include_str!("queries/javascript.scm"),
                containers: Self::JAVASCRIPT_CONTAINERS,
                functions: Self::JAVASCRIPT_FUNCTIONS,
            },
            LanguageSpec {
                name: "typescript",
                version: 1,
                extensions: &["ts", "mts", "cts"],
                grammar: || tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
                query: include_str!("queries/typescript.scm"),
                containers: Self::JAVASCRIPT_CONTAINERS,
                functions: Self::JAVASCRIPT_FUNCTIONS,
            },
            LanguageSpec {
                name: "tsx",
                version: 1,
                extensions: &["tsx"],
                grammar: || tree_sitter_typescript::LANGUAGE_TSX.into(),
                query: include_str!("queries/typescript.scm"),
                containers: Self::JAVASCRIPT_CONTAINERS,
                functions: Self::JAVASCRIPT_FUNCTIONS,
            },
            LanguageSpec {
                name: "go",
                version: 1,
                extensions: &["go"],
                grammar: || tree_sitter_go::LANGUAGE.into(),
                query: include_str!("queries/go.scm"),
                containers: &[],
                functions: &["function_declaration", "method_declaration", "func_literal"],
            },
            LanguageSpec {
                name: "java",
                version: 1,
                extensions: &["java"],
                grammar: || tree_sitter_java::LANGUAGE.into(),
                query: include_str!("queries/java.scm"),
                containers: &[
                    Container {
                        kind: "class_declaration",
                        field: "name",
                    },
                    Container {
                        kind: "interface_declaration",
                        field: "name",
                    },
                    Container {
                        kind: "enum_declaration",
                        field: "name",
                    },
                    Container {
                        kind: "record_declaration",
                        field: "name",
                    },
                ],
                functions: &["method_declaration", "constructor_declaration", "lambda_expression"],
            },
            LanguageSpec {
                name: "php",
                version: 1,
                extensions: &["php"],
                grammar: || tree_sitter_php::LANGUAGE_PHP.into(),
                query: include_str!("queries/php.scm"),
                containers: &[
                    Container {
                        kind: "class_declaration",
                        field: "name",
                    },
                    Container {
                        kind: "trait_declaration",
                        field: "name",
                    },
                    Container {
                        kind: "interface_declaration",
                        field: "name",
                    },
                    Container {
                        kind: "enum_declaration",
                        field: "name",
                    },
                ],
                functions: &[
                    "function_definition",
                    "method_declaration",
                    "anonymous_function",
                    "arrow_function",
                ],
            },
            LanguageSpec {
                name: "rust",
                version: 1,
                extensions: &["rs"],
                grammar: || tree_sitter_rust::LANGUAGE.into(),
                query: include_str!("queries/rust.scm"),
                containers: &[
                    Container {
                        kind: "impl_item",
                        field: "type",
                    },
                    Container {
                        kind: "trait_item",
                        field: "name",
                    },
                ],
                functions: &["function_item", "closure_expression"],
            },
            LanguageSpec {
                name: "ruby",
                version: 1,
                extensions: &["rb"],
                grammar: || tree_sitter_ruby::LANGUAGE.into(),
                query: include_str!("queries/ruby.scm"),
                containers: &[
                    Container {
                        kind: "class",
                        field: "name",
                    },
                    Container {
                        kind: "module",
                        field: "name",
                    },
                ],
                functions: &["method", "singleton_method", "lambda", "block", "do_block"],
            },
            LanguageSpec {
                name: "csharp",
                version: 1,
                extensions: &["cs"],
                grammar: || tree_sitter_c_sharp::LANGUAGE.into(),
                query: include_str!("queries/csharp.scm"),
                containers: &[
                    Container {
                        kind: "class_declaration",
                        field: "name",
                    },
                    Container {
                        kind: "struct_declaration",
                        field: "name",
                    },
                    Container {
                        kind: "interface_declaration",
                        field: "name",
                    },
                    Container {
                        kind: "record_declaration",
                        field: "name",
                    },
                ],
                functions: &[
                    "method_declaration",
                    "constructor_declaration",
                    "local_function_statement",
                    "lambda_expression",
                    "anonymous_method_expression",
                ],
            },
        ]
    }

    /// Every file extension s1grep reads, without the dot.
    pub fn extensions() -> Vec<String> {
        Self::PYTHON_EXTENSIONS
            .iter()
            .copied()
            .chain(Self::specs().iter().flat_map(|spec| spec.extensions.iter().copied()))
            .map(ToString::to_string)
            .collect()
    }
}
