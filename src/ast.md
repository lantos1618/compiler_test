
#### required expressions
- literals(int, float, bool, char, string)
- variables
- types
    - primitive(int, float, bool, string)
    - struct
        - struct_decl
        - struct_def
    - enum
        - enum_decl
        - enum_def
    - type_alias

- loops
    - break
    - continue
- if
    - else
- func
    - func_decl
    - func_def
    - func_call
    - return
- binary_operators
    - arithmetic(+, -, *, /, %)
    - logical(==, !=, >, >=, <, <=)
    - bitwise(&, |, ^, <<, >>)
- unary_operators(!, -)


#### grammar
program -> statements
statements -> [statement*]
statement -> expression_stmt | return_stmt | if_stmt | loop_stmt


```yaml

literal:
    - int
    - float
    - bool
    - char
    - string

ast_type:
    - I8, I16, I32, I64
    - U8, U16, U32, U64
    - F32, F64
    - Bool
    - Char
    - String
    - Struct
    - Enum
    - TypeAlias

binary_op:
    - Add, Sub, Mul, Div, Mod    # arithmetic
    - Eq, Ne, Gt, Ge, Lt, Le     # logical
    - BitAnd, BitOr, BitXor      # bitwise
    - Shl, Shr                   # shift

unary_op:
    - Not    # logical not
    - Neg    # arithmetic negation

struct_decl:
    name: String
    fields: [(String, AstType)]

struct_def:
    name: String
    fields: [(String, expr)]

enum_decl:
    name: String
    variants: [(String, Option<AstType>)]

enum_def:
    name: String
    variant: String
    value: Option<expr>

type_alias:
    name: String
    target: AstType

func_decl:
    name: String
    params: [(String, AstType)]
    return_type: Option<AstType>

func_def:
    decl: func_decl
    body: block

func_call:
    name: String
    args: [expr]

if_stmt:
    condition: expr
    then_branch: block
    else_branch: Option<block>

loop_stmt:
    condition: expr
    body: block

stmt:
    - var_decl
    - func_decl
    - func_def
    - func_call
    - assign
    - block
    - loop_stmt
    - if_stmt
    - expr
    - return
    - break
    - continue
    - struct_decl
    - struct_def
    - enum_decl
    - enum_def
    - type_alias

block: [stmt]

program:
    statements: block

variable:
    name: String
    type_: AstType

var_decl:
    name: String
    type_: AstType
    init: Option<expr>

assign:
    target: variable
    value: expr

expr:
    - literal
    - variable
    - ast_type
    - binary
    - unary
    - func_call
    - struct_def
    - enum_def

binary:
    op: binary_op
    left: expr
    right: expr

unary:
    op: unary_op
    expr: expr

return:
    value: Option<expr>
```
