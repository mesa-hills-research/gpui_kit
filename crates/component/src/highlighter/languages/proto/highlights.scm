; Protocol Buffers highlights for tree-sitter-proto 0.6.0, adapted from the
; grammar's own query (see NOTICE).

(comment) @comment

; Declarations

(package
  (full_ident
    (identifier) @namespace))

[
  (message_name)
  (enum_name)
  (service_name)
] @type

(rpc_name) @function

(oneof
  (identifier) @property)

(enum_field
  (identifier) @constant)

[
  (field
    (identifier) @property)
  (map_field
    (identifier) @property)
  (oneof_field
    (identifier) @property)
]

(extend
  (full_ident
    (identifier) @type))

; Types

[
  (key_type)
  (type)
  (message_or_enum_type)
] @type

; Options

[
  (option
    (identifier) @property)
  (option
    (full_ident
      (identifier) @property))
  (field_option
    (identifier) @property)
  (field_option
    (full_ident
      (identifier) @property))
  (enum_value_option
    (identifier) @property)
  (enum_value_option
    (full_ident
      (identifier) @property))
  (block_lit
    (identifier) @property)
  (extension_name
    (full_ident
      (identifier) @property))
]

(constant
  (full_ident
    (identifier) @constant))

; Literals

(syntax
  version: (string) @string.special)

(edition
  year: (string) @string.special)

(import
  path: (string) @string.special)

[
  (string)
  (reserved_identifier)
] @string

(escape_sequence) @string.escape

[
  (int_lit)
  (float_lit)
] @number

[
  (true)
  (false)
] @boolean

; Keywords

[
  "edition"
  "enum"
  "export"
  "extend"
  "extensions"
  "group"
  "import"
  "local"
  "map"
  "max"
  "message"
  "oneof"
  "option"
  "optional"
  "package"
  "public"
  "repeated"
  "required"
  "reserved"
  "returns"
  "rpc"
  "service"
  "stream"
  "syntax"
  "to"
  "weak"
] @keyword

; Punctuation

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
  "<"
  ">"
] @punctuation.bracket

[
  ";"
  ","
  "."
  ":"
  "/"
] @punctuation.delimiter

[
  "="
  "-"
  "+"
] @operator
