; CMake highlights for tree-sitter-cmake 0.7.5, adapted from the grammar's own
; query (see NOTICE).
;
; When several patterns capture the same node, the highlighter keeps the first,
; so the argument patterns go from the most specific to the least.

[
  (line_comment)
  (bracket_comment)
] @comment

; Control flow and definitions

[
  (if)
  (elseif)
  (else)
  (endif)
  (foreach)
  (endforeach)
  (while)
  (endwhile)
  (function)
  (endfunction)
  (macro)
  (endmacro)
  (block)
  (endblock)
] @keyword

(normal_command
  (identifier) @keyword
  (#match? @keyword "(?i)^(break|continue|return)$"))

; Commands

(normal_command
  (identifier) @function)

(function_command
  (argument_list
    .
    (argument) @function))

(macro_command
  (argument_list
    .
    (argument) @function))

[
  (function_command
    (argument_list
      (argument) @variable.parameter))
  (macro_command
    (argument_list
      (argument) @variable.parameter))
]

; Condition operators

([
  (if_command
    (argument_list
      (argument) @keyword))
  (elseif_command
    (argument_list
      (argument) @keyword))
  (while_command
    (argument_list
      (argument) @keyword))
]
  (#any-of? @keyword
    "NOT" "AND" "OR" "COMMAND" "POLICY" "TARGET" "TEST" "DEFINED" "IN_LIST" "EXISTS"
    "IS_NEWER_THAN" "IS_DIRECTORY" "IS_SYMLINK" "IS_ABSOLUTE" "IS_READABLE" "IS_WRITABLE"
    "IS_EXECUTABLE" "MATCHES" "LESS" "GREATER" "EQUAL" "LESS_EQUAL" "GREATER_EQUAL" "STRLESS"
    "STRGREATER" "STREQUAL" "STRLESS_EQUAL" "STRGREATER_EQUAL" "VERSION_LESS" "VERSION_GREATER"
    "VERSION_EQUAL" "VERSION_LESS_EQUAL" "VERSION_GREATER_EQUAL" "PATH_EQUAL"))

; Variables named by commands

(foreach_command
  (argument_list
    .
    (argument) @variable))

(normal_command
  (identifier) @_command
  (argument_list
    .
    (argument) @variable)
  (#match? @_command "(?i)^(set|unset|option)$"))

(normal_command
  (identifier) @_command
  (argument_list
    .
    (argument)
    .
    (argument) @variable)
  (#match? @_command "(?i)^(list|math)$"))

; Arguments

((unquoted_argument) @boolean
  (#match? @boolean "(?i)^(on|off|yes|no|true|false|y|n|ignore|notfound|.*-notfound)$"))

((unquoted_argument) @string.special
  (#match? @string.special "^\\$<"))

((unquoted_argument) @number
  (#match? @number "^-?\\d+(\\.\\d+)*$"))

((unquoted_argument) @constant
  (#match? @constant "^[A-Z@][A-Z\\d_]*$"))

[
  (quoted_argument)
  (bracket_argument)
] @string

(escape_sequence) @string.escape

; Variable references

(variable) @variable

[
  "ENV"
  "CACHE"
] @keyword

[
  (normal_var)
  (env_var)
  (cache_var)
] @embedded

[
  (normal_var
    [
      "$"
      "{"
      "}"
    ] @punctuation.special)
  (env_var
    [
      "$"
      "{"
      "}"
    ] @punctuation.special)
  (cache_var
    [
      "$"
      "{"
      "}"
    ] @punctuation.special)
]

[
  "("
  ")"
] @punctuation.bracket
