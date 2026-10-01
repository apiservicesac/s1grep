use std::collections::HashMap;

use tree_sitter::{Language, Node, Parser, Query, QueryCursor, StreamingIterator};

use crate::error::IndexError;
use crate::extractor::PythonExtractor;
use crate::settings::{IndexLimits, LanguageSettings};
use crate::unit::CodeUnit;

/// A node kind that encloses definitions and names them: a class, an `impl` block, a module.
pub struct Container {
    pub kind: &'static str,
    /// Field holding the container's name (`type` for a Rust `impl`).
    pub field: &'static str,
}

/// How one language is read: its grammar, which files are its own, and a tree-sitter query whose `@definition`
/// captures are the units and whose `@name` (and, for Go methods, `@receiver`) captures name them.
pub struct LanguageSpec {
    pub name: &'static str,
    /// Bumped whenever the query or the rules change, so files read with an older version are read again.
    pub version: u32,
    pub extensions: &'static [&'static str],
    pub grammar: fn() -> Language,
    pub query: &'static str,
    pub containers: &'static [Container],
    /// Kinds of function-like nodes: a definition inside one of them is a closure, not a unit.
    pub functions: &'static [&'static str],
}

impl LanguageSpec {
    pub fn extractor_version(&self) -> String {
        format!("{}-{}", self.name, self.version)
    }
}

/// One language read through its query.
struct QueryExtractor {
    spec: LanguageSpec,
    parser: Parser,
    query: Query,
}

impl QueryExtractor {
    fn new(spec: LanguageSpec) -> Result<Self, IndexError> {
        let language = (spec.grammar)();
        let mut parser = Parser::new();
        parser
            .set_language(&language)
            .map_err(|error| IndexError::Parser(format!("{}: {error}", spec.name)))?;
        let query = Query::new(&language, spec.query)
            .map_err(|error| IndexError::Parser(format!("{} query: {error}", spec.name)))?;
        Ok(Self { spec, parser, query })
    }

    fn extract(&mut self, relative_path: &str, source: &str) -> Vec<CodeUnit> {
        let Some(tree) = self.parser.parse(source, None) else {
            return Vec::new();
        };
        let names = self.query.capture_names();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&self.query, tree.root_node(), source.as_bytes());
        let mut units = Vec::new();
        while let Some(found) = matches.next() {
            let mut definition = None;
            let mut name = None;
            let mut receiver = None;
            for capture in found.captures() {
                match names[capture.index as usize] {
                    "definition" => definition = Some(capture.node),
                    "name" => name = Some(Self::text(capture.node, source)),
                    "receiver" => receiver = Some(Self::text(capture.node, source)),
                    _ => {}
                }
            }
            let (Some(definition), Some(name)) = (definition, name) else {
                continue;
            };
            if self.is_nested(definition) {
                continue;
            }
            let text = &source[definition.start_byte()..definition.end_byte()];
            if text.lines().count() < IndexLimits::MINIMUM_LINES {
                continue;
            }
            let mut qualifiers = self.containers(definition, source);
            qualifiers.extend(receiver);
            qualifiers.push(name);
            units.push(CodeUnit {
                path: relative_path.to_string(),
                name: qualifiers.join("."),
                start_line: definition.start_position().row + 1,
                end_line: definition.end_position().row + 1,
                source: text.trim_end().to_string(),
            });
        }
        units.sort_by_key(|unit| unit.start_line);
        units.dedup_by(|later, earlier| later.start_line == earlier.start_line && later.name == earlier.name);
        units
    }

    /// Whether the definition sits inside a function, i.e. is a closure or a helper local to it.
    fn is_nested(&self, definition: Node) -> bool {
        let mut ancestor = definition.parent();
        while let Some(node) = ancestor {
            if self.spec.functions.contains(&node.kind()) {
                return true;
            }
            ancestor = node.parent();
        }
        false
    }

    /// Names of the enclosing containers, outermost first: `Invoice.send`, `Service.Client.retry`.
    fn containers(&self, definition: Node, source: &str) -> Vec<String> {
        let mut names = Vec::new();
        let mut ancestor = definition.parent();
        while let Some(node) = ancestor {
            if let Some(container) = self
                .spec
                .containers
                .iter()
                .find(|container| container.kind == node.kind())
                && let Some(name) = node.child_by_field_name(container.field)
            {
                names.push(Self::text(name, source));
            }
            ancestor = node.parent();
        }
        names.reverse();
        names
    }

    fn text(node: Node, source: &str) -> String {
        source[node.start_byte()..node.end_byte()].to_string()
    }
}

/// Picks the extractor for each file by its extension and records which extractor version read it.
pub struct ExtractorRegistry {
    python: PythonExtractor,
    others: Vec<QueryExtractor>,
    by_extension: HashMap<&'static str, usize>,
}

impl ExtractorRegistry {
    pub fn new() -> Result<Self, IndexError> {
        let mut others = Vec::new();
        let mut by_extension = HashMap::new();
        for spec in LanguageSettings::specs() {
            for extension in spec.extensions {
                by_extension.insert(*extension, others.len());
            }
            others.push(QueryExtractor::new(spec)?);
        }
        Ok(Self {
            python: PythonExtractor::new()?,
            others,
            by_extension,
        })
    }

    /// The version of the extractor that reads `relative_path`, or `None` for a file no language claims.
    pub fn version_for(&self, relative_path: &str) -> Option<String> {
        let extension = Self::extension(relative_path)?;
        if LanguageSettings::PYTHON_EXTENSIONS.contains(&extension) {
            return Some(LanguageSettings::PYTHON_VERSION.to_string());
        }
        let position = self.by_extension.get(extension)?;
        Some(self.others[*position].spec.extractor_version())
    }

    pub fn extract(&mut self, relative_path: &str, source: &str) -> Vec<CodeUnit> {
        let Some(extension) = Self::extension(relative_path) else {
            return Vec::new();
        };
        if LanguageSettings::PYTHON_EXTENSIONS.contains(&extension) {
            return self.python.extract(relative_path, source);
        }
        match self.by_extension.get(extension) {
            Some(position) => self.others[*position].extract(relative_path, source),
            None => Vec::new(),
        }
    }

    fn extension(relative_path: &str) -> Option<&str> {
        relative_path
            .rsplit_once('/')
            .map_or(relative_path, |(_, file)| file)
            .rsplit_once('.')
            .map(|(_, extension)| extension)
    }
}

#[cfg(test)]
mod tests {
    use super::ExtractorRegistry;

    fn names(path: &str, source: &str) -> Vec<String> {
        let mut registry = ExtractorRegistry::new().expect("every query compiles");
        registry
            .extract(path, source)
            .into_iter()
            .map(|unit| unit.name)
            .collect()
    }

    #[test]
    fn javascript_functions_methods_and_bound_arrows_without_closures() {
        let source = "function send(invoice) {\n  const retry = () => {\n    return 1;\n  };\n  return retry();\n}\n\
                      class Gateway {\n  post(document) {\n    return fetch(document);\n  }\n}\n\
                      export const total = (lines) => {\n  return lines.reduce((sum, line) => sum + line, 0);\n};\n";
        assert_eq!(names("src/billing.js", source), ["send", "Gateway.post", "total"]);
    }

    #[test]
    fn typescript_and_tsx() {
        let source = "export class Client {\n  constructor(private url: string) {\n    this.url = url;\n  }\n  async get(path: string): Promise<string> {\n    return fetch(this.url + path).then((response) => response.text());\n  }\n}\n";
        assert_eq!(names("src/client.ts", source), ["Client.constructor", "Client.get"]);
        let component = "export function Invoice({ total }: Props) {\n  return (\n    <div>{total}</div>\n  );\n}\n";
        assert_eq!(names("src/Invoice.tsx", component), ["Invoice"]);
    }

    #[test]
    fn go_functions_and_methods_by_receiver() {
        let source = "package billing\n\nfunc Send(id int) error {\n\treturn nil\n}\n\nfunc (c *Client) Retry(n int) error {\n\tfor i := 0; i < n; i++ {\n\t}\n\treturn nil\n}\n";
        assert_eq!(names("billing/client.go", source), ["Send", "Client.Retry"]);
    }

    #[test]
    fn java_methods_and_constructors_in_their_class() {
        let source = "public class Invoice {\n  public Invoice(int total) {\n    this.total = total;\n  }\n  public int tax() {\n    return total / 10;\n  }\n  abstract void skip();\n}\n";
        assert_eq!(names("src/Invoice.java", source), ["Invoice.Invoice", "Invoice.tax"]);
    }

    #[test]
    fn php_functions_and_methods() {
        let source = "<?php\nfunction total($lines) {\n  return array_sum($lines);\n}\nclass Invoice {\n  public function send() {\n    return true;\n  }\n}\n";
        assert_eq!(names("app/invoice.php", source), ["total", "Invoice.send"]);
    }

    #[test]
    fn rust_functions_in_impl_and_trait_blocks() {
        let source = "fn main() {\n    let add = |a: i32| a + 1;\n    add(1);\n}\nimpl Invoice {\n    fn tax(&self) -> u32 {\n        self.total / 10\n    }\n}\n";
        assert_eq!(names("src/main.rs", source), ["main", "Invoice.tax"]);
    }

    #[test]
    fn ruby_methods_in_classes_and_modules() {
        let source = "module Billing\n  class Invoice\n    def send_now\n      deliver\n    end\n    def self.build(lines)\n      new(lines)\n    end\n  end\nend\n";
        assert_eq!(
            names("lib/invoice.rb", source),
            ["Billing.Invoice.send_now", "Billing.Invoice.build"]
        );
    }

    #[test]
    fn csharp_methods_and_constructors() {
        let source = "public class Invoice\n{\n    public Invoice(int total)\n    {\n        Total = total;\n    }\n    public int Tax()\n    {\n        return Total / 10;\n    }\n}\n";
        assert_eq!(names("Billing/Invoice.cs", source), ["Invoice.Invoice", "Invoice.Tax"]);
    }

    #[test]
    fn python_keeps_its_own_extractor_and_unknown_files_give_nothing() {
        let source = "class Invoice:\n    def tax(self):\n        value = 1\n        return value\n";
        assert_eq!(names("billing/invoice.py", source), ["Invoice.tax"]);
        assert!(names("README.md", "# Invoices\n\nsome\ntext\n").is_empty());
    }
}
