; Functions and methods of JavaScript and TypeScript: declarations, class methods, and arrow functions or function
; expressions bound to a name.
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
