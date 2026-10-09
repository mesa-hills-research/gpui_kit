[
  (comment)
  (haddock)
] @comment

[
  (string)
  (char)
] @string

[
  (integer)
  (float)
] @number

[
  "module"
  "where"
  "import"
  "data"
  "deriving"
  "let"
  "in"
  "do"
  "if"
  "then"
  "else"
  "case"
  "of"
] @keyword

(signature name: (variable) @function)
(function name: (variable) @function)
(bind name: (variable) @function)

[
  (constructor)
  (name)
  (module_id)
] @type

(operator) @operator
(variable) @variable
