/// What a search looks like, judged by its characters alone so that it works the same in every human language.
pub struct QueryShape;

impl QueryShape {
    /// Whether some word of the query is written like code: `snake_case`, `camelCase`, `module.function`, `f()`,
    /// `Type::method`, a path, or text in quotes or backticks. Such searches look for names, so the word index takes
    /// part in full; other searches are prose and use it only for exact phrases.
    pub fn looks_like_code(query: &str) -> bool {
        query.split_whitespace().any(Self::is_code_word)
    }

    fn is_code_word(word: &str) -> bool {
        let word = word.trim_end_matches(['?', '!', ',', ';', ':']);
        if word.starts_with(['`', '"', '\'']) || word.contains(['(', ')', '[', ']', '{', '}', '<', '>', '=']) {
            return true;
        }
        if word.contains("::") || word.contains("->") {
            return true;
        }
        Self::is_snake_case(word) || Self::is_camel_case(word) || Self::is_member_access(word) || Self::is_path(word)
    }

    /// `send_invoice`: an underscore between letters or digits.
    fn is_snake_case(word: &str) -> bool {
        let characters: Vec<char> = word.chars().collect();
        characters
            .windows(3)
            .any(|window| window[0].is_alphanumeric() && window[1] == '_' && window[2].is_alphanumeric())
    }

    /// `sendInvoice`: starts in lower case and has a capital later. Brand names (`GitHub`, `JavaScript`) start in
    /// upper case and are not taken for code.
    fn is_camel_case(word: &str) -> bool {
        word.chars().next().is_some_and(char::is_lowercase) && word.chars().skip(1).any(char::is_uppercase)
    }

    /// `client.retry`: words of two or more characters, starting with a letter, joined by dots. Abbreviations
    /// (`a.m.`, `e.g.`) and numbers (`3.5`) are not.
    fn is_member_access(word: &str) -> bool {
        let word = word.trim_end_matches('.');
        let parts: Vec<&str> = word.split('.').collect();
        parts.len() >= 2
            && parts.iter().all(|part| {
                part.chars().count() >= 2
                    && part.chars().next().is_some_and(char::is_alphabetic)
                    && part
                        .chars()
                        .all(|character| character.is_alphanumeric() || character == '_')
            })
    }

    /// `src/main.rs`, `./run.sh`: a slash with a file extension at the end or a leading `/`, `./`, `../`. Dates and
    /// choices (`dd/mm/aaaa`, `and/or`) are not.
    fn is_path(word: &str) -> bool {
        if !word.contains('/') {
            return false;
        }
        word.starts_with('/')
            || word.starts_with("./")
            || word.starts_with("../")
            || word.rsplit('/').next().is_some_and(|file| {
                file.rsplit_once('.')
                    .is_some_and(|(stem, extension)| !stem.is_empty() && (1..=5).contains(&extension.len()))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::QueryShape;

    #[test]
    fn tells_names_and_code_from_prose() {
        for code in [
            "send_invoice",
            "sendInvoice",
            "client.retry",
            "parse()",
            "Config::load",
            "src/main.rs",
            "`run`",
        ] {
            assert!(QueryShape::looks_like_code(code), "{code}");
        }
        for prose in [
            "where do we retry a failed payment",
            "dónde se valida el token del usuario?",
            "How are tokens refreshed? It matters.",
            "remove the webhook from GitHub and JavaScript clients",
            "convertir fechas dd/mm/aaaa a año-mes-día y/o ISO",
            "schedule the job at 3 a.m. for version 3.5, e.g. nightly",
        ] {
            assert!(!QueryShape::looks_like_code(prose), "{prose}");
        }
    }
}
