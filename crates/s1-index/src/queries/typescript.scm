; TypeScript adds nothing with a body to JavaScript's definitions; abstract and overload signatures have no code.
(function_declaration name: (identifier) @name) @definition
(generator_function_declaration name: (identifier) @name) @definition
(method_definition name: (_) @name) @definition
(lexical_declaration
  (variable_declarator
    name: (identifier) @name
    value: [(arrow_function) (function_expression)])) @definition
(variable_declaration
  (variable_declarator
    name: (identifier) @name
    value: [(arrow_function) (function_expression)])) @definition
