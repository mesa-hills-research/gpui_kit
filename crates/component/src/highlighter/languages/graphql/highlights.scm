; GraphQL highlights for tree-sitter-graphql 0.1.0, adapted from nvim-treesitter's
; graphql query (see NOTICE).

; Comments and descriptions

(comment) @comment

(description
  (string_value) @comment.doc)

; Literals

(string_value) @string

(int_value) @number

(float_value) @number

(boolean_value) @boolean

(null_value) @constant.builtin

; Operations and fragments

(operation_type) @keyword

(operation_definition
  (name) @function)

(fragment_name
  (name) @function)

; Variables

(variable_definition
  (variable) @variable.parameter)

(variable) @variable

; Types

[
  (scalar_type_definition
    (name) @type)
  (object_type_definition
    (name) @type)
  (interface_type_definition
    (name) @type)
  (union_type_definition
    (name) @type)
  (enum_type_definition
    (name) @type)
  (input_object_type_definition
    (name) @type)
  (scalar_type_extension
    (name) @type)
  (object_type_extension
    (name) @type)
  (interface_type_extension
    (name) @type)
  (union_type_extension
    (name) @type)
  (enum_type_extension
    (name) @type)
  (input_object_type_extension
    (name) @type)
]

(named_type
  (name) @type)

; Directives

(directive
  "@" @attribute
  (name) @attribute)

(directive_definition
  "@" @attribute
  (name) @attribute)

(directive_location) @constant

; Fields, arguments and values

(field
  (alias
    (name) @property))

(field
  (name) @property)

(field_definition
  (name) @property)

(input_fields_definition
  (input_value_definition
    (name) @property))

(arguments_definition
  (input_value_definition
    (name) @variable.parameter))

(argument
  (name) @variable.parameter)

(object_field
  (name) @property)

(enum_value
  (name) @constant)

; Keywords

[
  "directive"
  "enum"
  "extend"
  "fragment"
  "implements"
  "input"
  "interface"
  "on"
  "repeatable"
  "scalar"
  "schema"
  "type"
  "union"
] @keyword

; Punctuation

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
] @punctuation.bracket

[
  ":"
  (comma)
  "|"
  "&"
] @punctuation.delimiter

"=" @operator

[
  "!"
  "..."
] @punctuation.special
