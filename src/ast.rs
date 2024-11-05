use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Program {
    pub statements: Block,
}

pub type Block = Vec<Stmt>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Stmt {
    VarDecl(VarDecl),
    FuncDecl(FuncDecl),
    FuncDef(FuncDef),
    FuncCall(FuncCall),
    Assign(Assign),
    Block(Block),
    Loop(LoopStmt),
    If(IfStmt),
    Expr(Expr),
    Return(Return),
    Break,
    Continue,
    StructDecl(StructDecl),
    StructDef(StructDef),
    EnumDecl(EnumDecl),
    EnumDef(EnumDef),
    TypeAlias(TypeAlias),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AstType {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    Bool,
    Char,
    String,
    Struct(String),
    Enum(String),
    TypeAlias(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Literal {
    Int(i64),
    Float(f64),
    Bool(bool),
    Char(char),
    String(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BinaryOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    // Logical
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    // Bitwise
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum UnaryOp {
    Not,
    Neg,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructDecl {
    pub name: String,
    pub fields: Vec<(String, AstType)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<(String, Expr)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumDecl {
    pub name: String,
    pub variants: Vec<(String, Option<AstType>)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumDef {
    pub name: String,
    pub variant: String,
    pub value: Option<Box<Expr>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypeAlias {
    pub name: String,
    pub target: AstType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FuncDecl {
    pub name: String,
    pub params: Vec<(String, AstType)>,
    pub return_type: Option<AstType>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FuncDef {
    pub decl: FuncDecl,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FuncCall {
    pub name: String,
    pub args: Vec<Expr>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IfStmt {
    pub condition: Box<Expr>,
    pub then_branch: Block,
    pub else_branch: Option<Block>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoopStmt {
    pub condition: Box<Expr>,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Variable_ {
    pub name: String,
    pub type_: AstType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VarDecl {
    pub name: String,
    pub type_: AstType,
    pub init: Option<Box<Expr>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Assign {
    pub target: Variable_,
    pub value: Box<Expr>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    Literal(Literal),
    Variable(Variable_),
    Type(AstType),
    Binary(Box<Binary>),
    Unary(Box<Unary>),
    FuncCall(FuncCall),
    StructDef(StructDef),
    EnumDef(EnumDef),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Binary {
    pub op: BinaryOp,
    pub left: Box<Expr>,
    pub right: Box<Expr>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Unary {
    pub op: UnaryOp,
    pub expr: Box<Expr>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Return {
    pub value: Option<Box<Expr>>,
}

// Example usage:
impl Program {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}
