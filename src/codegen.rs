use crate::{
    module::ModuleType,
    ast::*,
};
use cranelift::prelude::*;
use cranelift_module::{FuncId, Linkage, ModuleResult};
use std::collections::HashMap;

pub struct Codegen {
    module: ModuleType,
    func_ctx: FunctionBuilderContext,
    functions: HashMap<String, FuncId>,
    variables: HashMap<String, Variable>,
}

impl Codegen {
    pub fn new(module: ModuleType) -> Self {
        Self {
            module,
            func_ctx: FunctionBuilderContext::new(),
            functions: HashMap::new(),
            variables: HashMap::new(),
        }
    }

    pub fn compile_program(&mut self, program: Program) -> ModuleResult<()> {
        for stmt in program.statements {
            self.compile_stmt(stmt)?;
        }
        Ok(())
    }

    fn compile_stmt(&mut self, stmt: Stmt) -> ModuleResult<()> {
        match stmt {
            Stmt::FuncDecl(func_decl) => self.declare_function(func_decl),
            Stmt::FuncDef(func_def) => self.define_function(func_def),
            Stmt::VarDecl(var_decl) => self.declare_variable(var_decl),
            Stmt::If(if_stmt) => {
                let mut ctx = self.module.make_context();
                let mut builder = FunctionBuilder::new(&mut ctx.func, &mut self.func_ctx);
                self.compile_if_stmt_in_func(if_stmt, &mut builder)
            },
            Stmt::Loop(loop_stmt) => {
                let mut ctx = self.module.make_context();
                let mut builder = FunctionBuilder::new(&mut ctx.func, &mut self.func_ctx);
                self.compile_loop_stmt_in_func(loop_stmt.clone(), &mut builder)
            },
            Stmt::Assign(assign) => {
                let mut ctx = self.module.make_context();
                let mut builder = FunctionBuilder::new(&mut ctx.func, &mut self.func_ctx);
                self.compile_assign(assign, &mut builder)
            },
            _ => unimplemented!("Statement type not yet implemented"),
        }
    }

    fn compile_standalone_stmt<T>(
        &mut self,
        stmt: T,
        compiler_fn: fn(&mut Self, T, &mut FunctionBuilder) -> ModuleResult<()>
    ) -> ModuleResult<()> {
        let mut ctx = self.module.make_context();
        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut self.func_ctx);
        compiler_fn(self, stmt, &mut builder)
    }

    fn declare_function(&mut self, func_decl: FuncDecl) -> ModuleResult<()> {
        let mut sig = self.module.make_signature();
        for (_name, param_type) in &func_decl.params {
            let abi_param = self.convert_type(param_type)?;
            sig.params.push(abi_param);
        }
        if let Some(return_type) = func_decl.return_type {
            sig.returns.push(self.convert_type(&return_type)?);
        }

        let func_id = self
            .module
            .declare_function(&func_decl.name, Linkage::Export, &sig)?;
        self.functions.insert(func_decl.name, func_id);
        Ok(())
    }

    fn define_function(&mut self, func_def: FuncDef) -> ModuleResult<()> {
        let func_id = self.functions.get(&func_def.decl.name).unwrap().to_owned();
        let mut ctx = self.module.make_context();
        ctx.func.signature = self.module.make_signature();

        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut self.func_ctx);
        let entry_block = builder.create_block();
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        let statements = func_def.body;
        for stmt in &statements {
            self.compile_stmt_in_func(stmt, &mut builder)?;
        }

        builder.finalize();
        self.module.define_function(func_id, &mut ctx)?;
        Ok(())
    }

    fn declare_variable(&mut self, var_decl: VarDecl) -> ModuleResult<()> {
        let var = Variable::new(self.variables.len());
        self.variables.insert(var_decl.name.clone(), var);
        Ok(())
    }

    fn compile_stmt_in_func(&mut self, stmt: &Stmt, builder: &mut FunctionBuilder) -> ModuleResult<()> {
        match stmt {
            Stmt::Return(ret) => self.compile_return(ret.clone(), builder),
            Stmt::Expr(expr) => {
                self.compile_expr(expr.clone(), builder);
                Ok(())
            }
            Stmt::If(if_stmt) => self.compile_if_stmt_in_func(if_stmt.clone(), builder),
            Stmt::Loop(loop_stmt) => self.compile_loop_stmt_in_func(loop_stmt, builder),
            _ => unimplemented!("Statement type not yet implemented in function body"),
        }
    }

    fn compile_return(
        &mut self,
        ret: Return,
        builder: &mut FunctionBuilder,
    ) -> ModuleResult<()> {
        if let Some(expr) = ret.value {
            let value = self.compile_expr(*expr, builder);
            builder.ins().return_(&[value]);
        } else {
            builder.ins().return_(&[]);
        }
        Ok(())
    }

    fn compile_if_stmt_in_func(
        &mut self,
        if_stmt: IfStmt,
        builder: &mut FunctionBuilder,
    ) -> ModuleResult<()> {
        let condition = self.compile_expr(*if_stmt.condition, builder);
        let then_block = builder.create_block();
        let else_block = builder.create_block();
        let merge_block = builder.create_block();

        builder
            .ins()
            .brif(condition, then_block, &[], else_block, &[]);

        // Then block
        builder.switch_to_block(then_block);
        for stmt in if_stmt.then_branch {
            self.compile_stmt_in_func(&stmt, builder)?;
        }
        builder.ins().jump(merge_block, &[]);
        builder.seal_block(then_block);

        // Else block
        builder.switch_to_block(else_block);
        if let Some(else_branch) = if_stmt.else_branch {
            for stmt in else_branch {
                self.compile_stmt_in_func(&stmt, builder)?;
            }
        }
        builder.ins().jump(merge_block, &[]);
        builder.seal_block(else_block);

        // Merge block
        builder.switch_to_block(merge_block);
        builder.seal_block(merge_block);

        Ok(())
    }

    fn compile_loop_stmt_in_func(
        &mut self,
        loop_stmt: LoopStmt,
        builder: &mut FunctionBuilder,
    ) -> ModuleResult<()> {
        let loop_header = builder.create_block();
        let loop_body = builder.create_block();
        let exit_block = builder.create_block();

        builder.ins().jump(loop_header, &[]);
        builder.switch_to_block(loop_header);

        let condition = self.compile_expr(*loop_stmt.condition, builder);
        builder
            .ins()
            .brif(condition, loop_body, &[], exit_block, &[]);

        builder.switch_to_block(loop_body);
        for stmt in &loop_stmt.body {
            self.compile_stmt_in_func(stmt, builder)?;
        }
        builder.ins().jump(loop_header, &[]);
        builder.seal_block(loop_body);

        builder.switch_to_block(exit_block);
        builder.seal_block(exit_block);

        Ok(())
    }

    fn compile_assign(
        &mut self,
        assign:  Assign,
        builder: &mut FunctionBuilder,
    ) -> ModuleResult<()> {
        let value = self.compile_expr(*assign.value, builder);
        let var = self.variables.get(&assign.target.name).unwrap();
        builder.def_var(*var, value);
        Ok(())
    }

    fn compile_expr(&mut self, expr: Expr, builder: &mut FunctionBuilder) -> Value {
        match expr {
            Expr::Literal(literal) => self.compile_literal(literal, builder),
            Expr::Variable(variable) => self.compile_variable(variable, builder),
            Expr::Binary(binary) => self.compile_binary(*binary, builder),
            Expr::Unary(unary) => self.compile_unary(*unary, builder),
            Expr::FuncCall(func_call) => self.compile_func_call(func_call, builder),
            _ => unimplemented!("Expression type not yet implemented"),
        }
    }

    fn compile_literal(&self, literal: Literal, builder: &mut FunctionBuilder) -> Value {
        match literal {
            Literal::Int(value) => builder.ins().iconst(types::I64, value),
            Literal::Float(value) => builder.ins().f64const(value),
            Literal::Bool(value) => builder.ins().iconst(types::I8, value as i64),
            _ => unimplemented!("Literal type not yet implemented"),
        }
    }

    fn compile_variable(&self, variable: Variable_, builder: &mut FunctionBuilder) -> Value {
        let var = *self.variables.get(&variable.name).unwrap();
        builder.use_var(var)
    }

    fn compile_binary(&mut self, binary: Binary, builder: &mut FunctionBuilder) -> Value {
        let left = self.compile_expr(*binary.left, builder);
        let right = self.compile_expr(*binary.right, builder);
        match binary.op {
            BinaryOp::Add => builder.ins().iadd(left, right),
            BinaryOp::Sub => builder.ins().isub(left, right),
            BinaryOp::Mul => builder.ins().imul(left, right),
            BinaryOp::Div => builder.ins().sdiv(left, right),
            BinaryOp::Eq => builder.ins().icmp(IntCC::Equal, left, right),
            BinaryOp::Ne => builder.ins().icmp(IntCC::NotEqual, left, right),
            BinaryOp::Gt => builder.ins().icmp(IntCC::SignedGreaterThan, left, right),
            BinaryOp::Lt => builder.ins().icmp(IntCC::SignedLessThan, left, right),
            _ => unimplemented!("Binary operation not yet implemented"),
        }
    }

    fn compile_unary(&mut self, unary: Unary, builder: &mut FunctionBuilder) -> Value {
        let expr = self.compile_expr(*unary.expr, builder);
        match unary.op {
            UnaryOp::Neg => builder.ins().ineg(expr),
            UnaryOp::Not => builder.ins().bnot(expr),
        }
    }

    fn compile_func_call(&mut self, func_call: FuncCall, builder: &mut FunctionBuilder) -> Value {
        let func_id = *self.functions.get(&func_call.name).unwrap();
        let func_ref = &self.module.declare_func_in_func(func_id, &mut builder.func);
        let args: Vec<Value> = func_call
            .args
            .into_iter()
            .map(|arg| self.compile_expr(arg, builder))
            .collect();
        let call = builder.ins().call(*func_ref, &args);
        builder.inst_results(call)[0]
    }

    fn convert_type(&self, ast_type: &AstType) -> ModuleResult<AbiParam> {
        let cranelift_type = match ast_type {
            AstType::I8 => types::I8,
            AstType::I16 => types::I16,
            AstType::I32 => types::I32,
            AstType::I64 => types::I64,
            // AstType::U8 => types::U8,
            // AstType::U16 => types::U16,
            // AstType::U32 => types::U32,
            // AstType::U64 => types::U64,
            AstType::F32 => types::F32,
            AstType::F64 => types::F64,
            AstType::Bool => types::I8,
            _ => unimplemented!("Type not yet implemented"),
        };
        Ok(AbiParam::new(cranelift_type))
    }
}
