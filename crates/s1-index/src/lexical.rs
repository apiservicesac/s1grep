use std::collections::HashSet;
use std::path::Path;

use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, BoostQuery, Occur, PhraseQuery, Query, TermQuery};
use tantivy::schema::{
    Field, IndexRecordOption, NumericOptions, STORED, STRING, Schema, TextFieldIndexing, TextOptions, Value,
};
use tantivy::tokenizer::{TextAnalyzer, Token, TokenStream, Tokenizer};
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument, Term, doc};

use crate::error::IndexError;
use crate::settings::LexicalSettings;
use crate::store::StoredUnit;

/// Splits code into searchable words the way identifiers are written: `sendInvoice`, `send_invoice` and
/// `SEND-INVOICE` all give `send` and `invoice`, plus the whole identifier (`sendinvoice`) so an exact name still
/// matches. Accents are folded and nothing is stemmed or dropped: the index knows no human language.
#[derive(Clone, Default)]
pub struct CodeTokenizer;

impl CodeTokenizer {
    pub const NAME: &'static str = "code";

    /// The words of `text`, in order, each with its position.
    pub fn words(text: &str) -> Vec<String> {
        let mut words = Vec::new();
        let mut identifier = String::new();
        for character in text.chars().chain(std::iter::once(' ')) {
            if character.is_alphanumeric() || character == '_' {
                identifier.push(character);
            } else if !identifier.is_empty() {
                Self::push_identifier(&identifier, &mut words);
                identifier.clear();
            }
        }
        words
    }

    fn push_identifier(identifier: &str, words: &mut Vec<String>) {
        let parts = Self::parts(identifier);
        if parts.len() > 1 {
            words.push(Self::fold(&identifier.replace('_', "")));
        }
        words.extend(parts.iter().map(|part| Self::fold(part)));
    }

    /// The pieces of one identifier: split at underscores, at a lower-to-upper change, before the last capital of
    /// an acronym followed by lower case (`HTTPServer` → `HTTP`, `Server`), and between letters and digits.
    fn parts(identifier: &str) -> Vec<String> {
        let characters: Vec<char> = identifier.chars().collect();
        let mut parts = Vec::new();
        let mut current = String::new();
        for (position, &character) in characters.iter().enumerate() {
            if character == '_' {
                if !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                }
                continue;
            }
            if let Some(&previous) = position.checked_sub(1).and_then(|before| characters.get(before)) {
                let next = characters.get(position + 1).copied();
                let boundary = (previous.is_lowercase() && character.is_uppercase())
                    || (previous.is_uppercase() && character.is_uppercase() && next.is_some_and(char::is_lowercase))
                    || (previous.is_alphabetic() && character.is_numeric())
                    || (previous.is_numeric() && character.is_alphabetic());
                if boundary && !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                }
            }
            current.push(character);
        }
        if !current.is_empty() {
            parts.push(current);
        }
        parts
    }

    /// Lower case without accents, so `Configuración` and `configuracion` meet.
    fn fold(word: &str) -> String {
        word.chars()
            .flat_map(char::to_lowercase)
            .map(|character| match character {
                'á' | 'à' | 'â' | 'ä' | 'ã' => 'a',
                'é' | 'è' | 'ê' | 'ë' => 'e',
                'í' | 'ì' | 'î' | 'ï' => 'i',
                'ó' | 'ò' | 'ô' | 'ö' | 'õ' => 'o',
                'ú' | 'ù' | 'û' | 'ü' => 'u',
                'ñ' => 'n',
                'ç' => 'c',
                other => other,
            })
            .collect()
    }
}

/// The words of one text as tantivy reads them.
pub struct CodeTokenStream {
    tokens: Vec<Token>,
    position: usize,
}

impl TokenStream for CodeTokenStream {
    fn advance(&mut self) -> bool {
        self.position += 1;
        self.position <= self.tokens.len()
    }

    fn token(&self) -> &Token {
        &self.tokens[self.position - 1]
    }

    fn token_mut(&mut self) -> &mut Token {
        &mut self.tokens[self.position - 1]
    }
}

impl Tokenizer for CodeTokenizer {
    type TokenStream<'a> = CodeTokenStream;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> CodeTokenStream {
        let tokens = Self::words(text)
            .into_iter()
            .enumerate()
            .map(|(position, text)| Token {
                position,
                text,
                ..Token::default()
            })
            .collect();
        CodeTokenStream { tokens, position: 0 }
    }
}

/// The fields of one indexed function.
#[derive(Clone, Copy)]
struct LexicalFields {
    unit_id: Field,
    file: Field,
    path: Field,
    name: Field,
    body: Field,
}

/// One project's word index: BM25 over each function's name, path and code. It mirrors the catalog: files that
/// changed are replaced, and an index that does not match the catalog (an older format, a crash between the two
/// writes) is rebuilt, which takes seconds.
pub struct LexicalIndex {
    index: Index,
    reader: IndexReader,
    fields: LexicalFields,
}

/// A function the word index found.
#[derive(Debug, Clone)]
pub struct LexicalHit {
    pub unit_id: i64,
    pub path: String,
    pub score: f32,
}

impl LexicalIndex {
    pub fn open(folder: &Path) -> Result<Self, IndexError> {
        let format_file = folder.join(LexicalSettings::FORMAT_FILE);
        let current = std::fs::read_to_string(&format_file).ok();
        if current.as_deref() != Some(LexicalSettings::FORMAT) && folder.exists() {
            std::fs::remove_dir_all(folder).map_err(|source| IndexError::Io {
                path: folder.to_path_buf(),
                source,
            })?;
        }
        std::fs::create_dir_all(folder).map_err(|source| IndexError::Io {
            path: folder.to_path_buf(),
            source,
        })?;
        let (schema, fields) = Self::schema();
        let directory = tantivy::directory::MmapDirectory::open(folder).map_err(Self::error)?;
        let index = Index::open_or_create(directory, schema).map_err(Self::error)?;
        index
            .tokenizers()
            .register(CodeTokenizer::NAME, TextAnalyzer::builder(CodeTokenizer).build());
        std::fs::write(&format_file, LexicalSettings::FORMAT).map_err(|source| IndexError::Io {
            path: format_file.clone(),
            source,
        })?;
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()
            .map_err(Self::error)?;
        Ok(Self { index, reader, fields })
    }

    pub fn document_count(&self) -> u64 {
        self.reader.searcher().num_docs()
    }

    /// Replaces the whole index with `units`.
    pub fn rebuild(&mut self, units: &[StoredUnit]) -> Result<(), IndexError> {
        let writer = self.writer()?;
        writer.delete_all_documents().map_err(Self::error)?;
        for unit in units {
            writer.add_document(self.document(unit)).map_err(Self::error)?;
        }
        self.commit(writer)
    }

    /// Replaces the functions of the files in `paths` with `units` (the files' new functions; none for a file that
    /// was removed).
    pub fn replace_files(&mut self, paths: &[String], units: &[StoredUnit]) -> Result<(), IndexError> {
        if paths.is_empty() {
            return Ok(());
        }
        let writer = self.writer()?;
        for path in paths {
            writer.delete_term(Term::from_field_text(self.fields.file, path));
        }
        for unit in units {
            writer.add_document(self.document(unit)).map_err(Self::error)?;
        }
        self.commit(writer)
    }

    /// The `limit` best functions for `query` among those whose path `keep` accepts, best first.
    pub fn search(
        &self,
        query: &str,
        limit: usize,
        keep: impl Fn(&str) -> bool,
    ) -> Result<Vec<LexicalHit>, IndexError> {
        let Some(query) = self.query(query) else {
            return Ok(Vec::new());
        };
        self.collect(&query, limit, keep)
    }

    fn collect(
        &self,
        query: &dyn Query,
        limit: usize,
        keep: impl Fn(&str) -> bool,
    ) -> Result<Vec<LexicalHit>, IndexError> {
        let searcher = self.reader.searcher();
        let found = searcher
            .search(
                query,
                &TopDocs::with_limit(limit * LexicalSettings::OVERFETCH).order_by_score(),
            )
            .map_err(Self::error)?;
        let mut hits = Vec::new();
        for (score, address) in found {
            let document: TantivyDocument = searcher.doc(address).map_err(Self::error)?;
            let path = document
                .get_first(self.fields.file)
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string();
            if !keep(&path) {
                continue;
            }
            let unit_id = document
                .get_first(self.fields.unit_id)
                .and_then(|value| value.as_i64())
                .unwrap_or_default();
            hits.push(LexicalHit { unit_id, path, score });
            if hits.len() == limit {
                break;
            }
        }
        Ok(hits)
    }

    /// The functions that contain `text` word for word (a pasted error message, a line of code), best first. Fewer
    /// than two words is not a phrase and finds nothing.
    pub fn search_phrase(
        &self,
        text: &str,
        limit: usize,
        keep: impl Fn(&str) -> bool,
    ) -> Result<Vec<LexicalHit>, IndexError> {
        let words = CodeTokenizer::words(text);
        if words.len() < LexicalSettings::MINIMUM_PHRASE_WORDS {
            return Ok(Vec::new());
        }
        let terms: Vec<(usize, Term)> = words
            .iter()
            .enumerate()
            .map(|(position, word)| (position, Term::from_field_text(self.fields.body, word)))
            .collect();
        self.collect(&PhraseQuery::new_with_offset(terms), limit, keep)
    }

    /// Every word of the query, in the name, path and code fields with their boosts; any of them may match.
    fn query(&self, text: &str) -> Option<BooleanQuery> {
        let mut seen = HashSet::new();
        let mut clauses: Vec<(Occur, Box<dyn Query>)> = Vec::new();
        for word in CodeTokenizer::words(text) {
            if word.chars().count() < LexicalSettings::MINIMUM_TERM_LENGTH || !seen.insert(word.clone()) {
                continue;
            }
            for (field, boost) in [
                (self.fields.name, LexicalSettings::NAME_BOOST),
                (self.fields.path, LexicalSettings::PATH_BOOST),
                (self.fields.body, LexicalSettings::BODY_BOOST),
            ] {
                let term = TermQuery::new(Term::from_field_text(field, &word), IndexRecordOption::WithFreqs);
                clauses.push((Occur::Should, Box::new(BoostQuery::new(Box::new(term), boost))));
            }
        }
        (!clauses.is_empty()).then(|| BooleanQuery::new(clauses))
    }

    fn document(&self, stored: &StoredUnit) -> TantivyDocument {
        doc!(
            self.fields.unit_id => stored.id,
            self.fields.file => stored.unit.path.clone(),
            self.fields.path => stored.unit.path.clone(),
            self.fields.name => stored.unit.name.clone(),
            self.fields.body => stored.unit.source.clone(),
        )
    }

    fn writer(&self) -> Result<IndexWriter, IndexError> {
        self.index
            .writer_with_num_threads(1, LexicalSettings::WRITER_MEMORY)
            .map_err(Self::error)
    }

    fn commit(&mut self, mut writer: IndexWriter) -> Result<(), IndexError> {
        writer.commit().map_err(Self::error)?;
        writer.wait_merging_threads().map_err(Self::error)?;
        self.reader.reload().map_err(Self::error)?;
        Ok(())
    }

    fn schema() -> (Schema, LexicalFields) {
        let mut builder = Schema::builder();
        let words = TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(CodeTokenizer::NAME)
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        );
        let fields = LexicalFields {
            unit_id: builder.add_i64_field("unit_id", NumericOptions::default() | STORED),
            file: builder.add_text_field("file", STRING | STORED),
            path: builder.add_text_field("path", words.clone()),
            name: builder.add_text_field("name", words.clone()),
            body: builder.add_text_field("body", words),
        };
        (builder.build(), fields)
    }

    fn error(error: impl std::fmt::Display) -> IndexError {
        IndexError::Lexical(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{CodeTokenizer, LexicalIndex};
    use crate::store::StoredUnit;
    use crate::unit::CodeUnit;

    fn stored(id: i64, path: &str, name: &str, source: &str) -> StoredUnit {
        StoredUnit {
            id,
            content: format!("content-{id}"),
            unit: CodeUnit {
                path: path.to_string(),
                name: name.to_string(),
                start_line: 1,
                end_line: 3,
                source: source.to_string(),
            },
        }
    }

    #[test]
    fn finds_by_name_replaces_changed_files_and_filters_by_folder() {
        let folder = std::env::temp_dir().join(format!("s1-index-lexical-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let mut index = LexicalIndex::open(&folder).unwrap();
        index
            .rebuild(&[
                stored(
                    1,
                    "billing/gateway.py",
                    "Gateway.send_invoice",
                    "def send_invoice(self):\n    return post()",
                ),
                stored(
                    2,
                    "auth/tokens.py",
                    "verify_token",
                    "def verify_token(token):\n    return decode(token)",
                ),
                stored(
                    3,
                    "legacy/gateway.py",
                    "send_invoice_v1",
                    "def send_invoice_v1():\n    return None",
                ),
            ])
            .unwrap();
        assert_eq!(index.document_count(), 3);
        let hits = index.search("sendInvoice", 5, |_| true).unwrap();
        assert_eq!(hits[0].unit_id, 1);
        let in_billing = index
            .search("send invoice", 5, |path| path.starts_with("billing/"))
            .unwrap();
        assert_eq!(in_billing.iter().map(|hit| hit.unit_id).collect::<Vec<_>>(), vec![1]);
        index
            .replace_files(
                &["auth/tokens.py".to_string()],
                &[stored(
                    4,
                    "auth/tokens.py",
                    "refresh_token",
                    "def refresh_token():\n    return new()",
                )],
            )
            .unwrap();
        assert_eq!(index.document_count(), 3);
        assert!(index.search("verify", 5, |_| true).unwrap().is_empty());
        assert_eq!(index.search("refresh token", 5, |_| true).unwrap()[0].unit_id, 4);
        drop(index);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn splits_identifiers_and_keeps_the_whole_name() {
        assert_eq!(
            CodeTokenizer::words("def sendRequestToAPI(http_client): Configuración v2"),
            [
                "def",
                "sendrequesttoapi",
                "send",
                "request",
                "to",
                "api",
                "httpclient",
                "http",
                "client",
                "configuracion",
                "v2",
                "v",
                "2"
            ]
        );
        assert_eq!(CodeTokenizer::words("HTTPServer"), ["httpserver", "http", "server"]);
    }
}
