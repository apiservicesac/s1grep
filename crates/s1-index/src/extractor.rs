use tree_sitter::{Node, Parser};

use crate::error::IndexError;
use crate::unit::CodeUnit;

/// Splits Python source into code units with the same rule used to build the training data and the exams:
/// top-level functions and methods defined directly in a top-level class, at least three lines long.
pub struct PythonExtractor {
    parser: Parser,
}

impl PythonExtractor {
    const MINIMUM_LINES: usize = 3;

    pub fn new() -> Result<Self, IndexError> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .map_err(|error| IndexError::Parser(error.to_string()))?;
        Ok(Self { parser })
    }

    pub fn extract(&mut self, relative_path: &str, source: &str) -> Vec<CodeUnit> {
        let Some(tree) = self.parser.parse(source, None) else {
            return Vec::new();
        };
        let mut units = Vec::new();
        let root = tree.root_node();
        let mut cursor = root.walk();
        for child in root.named_children(&mut cursor) {
            let definition = Self::unwrap_decorated(child);
            match definition.kind() {
                "function_definition" => self.push_unit(&mut units, relative_path, source, definition, None),
                "class_definition" => self.push_methods(&mut units, relative_path, source, definition),
                _ => {}
            }
        }
        units
    }

    fn push_methods(&self, units: &mut Vec<CodeUnit>, relative_path: &str, source: &str, class: Node) {
        let Some(class_name) = Self::field_text(class, "name", source) else {
            return;
        };
        let Some(body) = class.child_by_field_name("body") else {
            return;
        };
        let mut cursor = body.walk();
        for child in body.named_children(&mut cursor) {
            let definition = Self::unwrap_decorated(child);
            if definition.kind() == "function_definition" {
                self.push_unit(units, relative_path, source, definition, Some(&class_name));
            }
        }
    }

    fn push_unit(&self, units: &mut Vec<CodeUnit>, relative_path: &str, source: &str, function: Node, class: Option<&str>) {
        let Some(function_name) = Self::field_text(function, "name", source) else {
            return;
        };
        let text = &source[function.start_byte()..function.end_byte()];
        if text.lines().count() < Self::MINIMUM_LINES {
            return;
        }
        let name = match class {
            Some(class) => format!("{class}.{function_name}"),
            None => function_name,
        };
        units.push(CodeUnit {
            path: relative_path.to_string(),
            name,
            start_line: function.start_position().row + 1,
            end_line: function.end_position().row + 1,
            source: text.trim_end().to_string(),
        });
    }

    /// `@decorator def f` is a `decorated_definition` wrapping the real definition; the unit starts at `def`.
    fn unwrap_decorated(node: Node) -> Node {
        if node.kind() == "decorated_definition" {
            if let Some(inner) = node.child_by_field_name("definition") {
                return inner;
            }
        }
        node
    }

    fn field_text(node: Node, field: &str, source: &str) -> Option<String> {
        node.child_by_field_name(field).map(|child| source[child.start_byte()..child.end_byte()].to_string())
    }
}
