; Functions, and methods named after their receiver type.
(function_declaration name: (identifier) @name) @definition
(method_declaration
  receiver: (parameter_list
    (parameter_declaration
      type: [(type_identifier) @receiver
             (pointer_type (type_identifier) @receiver)
             (generic_type type: (type_identifier) @receiver)
             (pointer_type (generic_type type: (type_identifier) @receiver))]))
  name: (field_identifier) @name) @definition
