use std::collections::HashMap;
use delegate::delegate;
use anyhow::Result;
use cranelift::prelude::*;
use cranelift_jit::{JITModule, JITBuilder};
use cranelift_module::{FuncId, DataId, Module, Linkage, ModuleResult};
use cranelift_object::ObjectModule;
use cranelift_codegen::*;
use target_lexicon;
use cranelift_codegen::ir::condcodes::{IntCC, FloatCC};


pub struct Codegen {
    pub module: ModuleType,
    pub functions: HashMap<String, FuncId>,
}

pub enum ModuleType {
    JITModule(JITModule),
    ObjectModule(ObjectModule),
}

impl ModuleType {
    delegate! {
        to match self {
            Self::JITModule(jit) => jit,
            Self::ObjectModule(obj) => obj,
        } {
            fn isa(&self) -> &dyn cranelift_codegen::isa::TargetIsa;
            fn declarations(&self) -> &cranelift_module::ModuleDeclarations;
            fn declare_function(&mut self, name: &str, linkage: Linkage, signature: &Signature) -> ModuleResult<FuncId>;
            fn declare_anonymous_function(&mut self, signature: &Signature) -> ModuleResult<FuncId>;
            fn declare_data(&mut self, name: &str, linkage: Linkage, writable: bool, tls: bool) -> ModuleResult<DataId>;
            fn declare_anonymous_data(&mut self, writable: bool, tls: bool) -> ModuleResult<DataId>;
            fn define_function(&mut self, id: FuncId, ctx: &mut cranelift_codegen::Context) -> ModuleResult<()>;
            fn define_data(&mut self, id: DataId, data: &cranelift_module::DataDescription) -> ModuleResult<()>;
            fn make_signature(&self) -> Signature;
            fn make_context(&self) -> cranelift_codegen::Context;
            fn clear_context(&self, ctx: &mut cranelift_codegen::Context);
            fn clear_signature(&self, sig: &mut Signature);
            fn target_config(&self) -> cranelift_codegen::isa::TargetFrontendConfig;
        }
    }
}

impl Codegen {
    pub fn new(module: ModuleType) -> Self {
        Self { 
            module,
            functions: HashMap::new(),
        }
    }

    pub fn run<T>(&self, func: String) -> Result<T> {
        let func_id = self.functions.get(&func).unwrap();
        
        let func_ptr = match &self.module {
            ModuleType::JITModule(jit) => jit.get_finalized_function(*func_id),
            ModuleType::ObjectModule(_) => return Err(anyhow::anyhow!("Cannot run functions from object module")),
        };
        
        let func: fn() -> T = unsafe { std::mem::transmute(func_ptr) };
        Ok(func())
    }

    pub fn run_main<T>(&self) -> Result<T> {
        self.run("main".to_string())
    }
}

#[cfg(test)]
mod tests {

    use cranelift_codegen::ir::{types, AbiParam, Function, Type};
    use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
    use cranelift_module::{Linkage, Module};

    use super::*;
    fn get_compiler() -> Result<Codegen> {
        let mut flags_builder = cranelift_codegen::settings::builder();
        let shared_flags = cranelift_codegen::settings::Flags::new(flags_builder);

        let triple = target_lexicon::HOST;
        let isa_builder = cranelift_native::builder().unwrap();
        let isa = isa_builder.finish(shared_flags)?;

        let libcall_names = cranelift_module::default_libcall_names();
        let jit_builder = JITBuilder::with_isa(isa, libcall_names);
        let jit_module = JITModule::new(jit_builder);
        
        let codegen = Codegen::new(ModuleType::JITModule(jit_module));
        Ok(codegen)
    }
    #[test]
    fn test_return_i32() {
        // i32 main() { return 0; }
        let mut codegen = get_compiler().unwrap();
    
        // Define function signature with i32 return type
        let mut signature = codegen.module.make_signature();
        signature.returns.push(AbiParam::new(types::I32));
    
        // Assign the signature to the function
        let mut func = Function::new();
        func.signature = signature.clone();  // <-- Add this line
    
        let mut func_builder_ctx = FunctionBuilderContext::new();
        let mut func_builder = FunctionBuilder::new(&mut func, &mut func_builder_ctx);
    
        let block = func_builder.create_block();
        func_builder.switch_to_block(block);
        func_builder.seal_block(block);
    
        // Return 0
        let zero = func_builder.ins().iconst(types::I32, 0);
        func_builder.ins().return_(&[zero]);
        func_builder.finalize();
    
        // Print the CLIF IR
        println!("Function CLIF IR:\n{}", func.display());
    
        // Declare function
        let func_id = codegen.module.declare_function("main", Linkage::Export, &signature).unwrap();
    
        // Create context and assign function
        let mut ctx = codegen.module.make_context();
        ctx.func = func;
    
        // Define the function
        codegen.module.define_function(func_id, &mut ctx).unwrap();

    
        // Finalize function definitions
        match &mut codegen.module {
            ModuleType::JITModule(jit) => jit.finalize_definitions().unwrap(),
            ModuleType::ObjectModule(_) => panic!("Cannot finalize definitions in object module"),
        }
    
        // Run main and assert result
        codegen.functions.insert("main".to_string(), func_id);
        let result = codegen.run_main::<i32>().unwrap();
        assert_eq!(result, 0);
    }

    #[test]
    fn test_if_else() {
        // i32 main() { if (0) { return 1; } else { return 0; } }
        let mut codegen = get_compiler().unwrap();

        let mut signature = codegen.module.make_signature();
        signature.returns.push(AbiParam::new(types::I32));  

        let mut func = Function::new();
        func.signature = signature.clone();

        let mut func_builder_ctx = FunctionBuilderContext::new();
        let mut func_builder = FunctionBuilder::new(&mut func, &mut func_builder_ctx);

        // Create the basic blocks
        let entry_block = func_builder.create_block();
        let true_block = func_builder.create_block();
        let false_block = func_builder.create_block();

        // Switch to entry block
        func_builder.switch_to_block(entry_block);
        
        // Create condition (0)
        let zero = func_builder.ins().iconst(types::I32, 0);
        
        // Create the if-else branch
        func_builder.ins().brif(zero, true_block, &[], false_block, &[]);
        
        // True block
        func_builder.switch_to_block(true_block);
        let one = func_builder.ins().iconst(types::I32, 1);
        func_builder.ins().return_(&[one]);
        
        // False block
        func_builder.switch_to_block(false_block);
        let zero_return = func_builder.ins().iconst(types::I32, 0);
        func_builder.ins().return_(&[zero_return]);
        
        // Seal all blocks
        func_builder.seal_block(entry_block);
        func_builder.seal_block(true_block);
        func_builder.seal_block(false_block);
        
        func_builder.finalize();

        // Print the CLIF IR
        println!("Function CLIF IR:\n{}", func.display());

        // Declare function
        let func_id = codegen.module.declare_function("main", Linkage::Export, &signature).unwrap();

        // Create context and assign function
        let mut ctx = codegen.module.make_context();
        ctx.func = func;

        // Define the function
        codegen.module.define_function(func_id, &mut ctx).unwrap();

        // Finalize function definitions
        match &mut codegen.module {
            ModuleType::JITModule(jit) => jit.finalize_definitions().unwrap(),
            ModuleType::ObjectModule(_) => panic!("Cannot finalize definitions in object module"),
        }

        // Run main and assert result
        codegen.functions.insert("main".to_string(), func_id);
        let result = codegen.run_main::<i32>().unwrap();
        assert_eq!(result, 0); // Since condition is 0 (false), it should return 0
    }


    #[test]
    fn test_while() {  
        // int main() { int i = 0; while (i < 10) { i++; } return i; }
        let mut codegen = get_compiler().unwrap();  

        let mut signature = codegen.module.make_signature();
        signature.returns.push(AbiParam::new(types::I32));

        let mut func = Function::new();
        func.signature = signature.clone(); 

        let mut func_builder_ctx = FunctionBuilderContext::new();
        let mut func_builder = FunctionBuilder::new(&mut func, &mut func_builder_ctx);

        // Create basic blocks
        let entry_block = func_builder.create_block();
        let loop_header = func_builder.create_block();
        let loop_body = func_builder.create_block();
        let exit_block = func_builder.create_block();

        // Start with entry block
        func_builder.switch_to_block(entry_block);
        
        // Create a variable slot for our counter
        let i_var = Variable::new(0);
        func_builder.declare_var(i_var, types::I32);
        
        // Initialize i = 0
        let zero = func_builder.ins().iconst(types::I32, 0);
        func_builder.def_var(i_var, zero);
        
        // Jump to loop header
        func_builder.ins().jump(loop_header, &[]);

        // Loop header: check condition (i < 10)
        func_builder.switch_to_block(loop_header);
        let i_val = func_builder.use_var(i_var);
        let ten = func_builder.ins().iconst(types::I32, 10);
        let condition = func_builder.ins().icmp(IntCC::SignedLessThan, i_val, ten);
        func_builder.ins().brif(condition, loop_body, &[], exit_block, &[]);

        // Loop body: increment i
        func_builder.switch_to_block(loop_body);
        let i_val = func_builder.use_var(i_var);
        let one = func_builder.ins().iconst(types::I32, 1);
        let i_plus_one = func_builder.ins().iadd(i_val, one);
        func_builder.def_var(i_var, i_plus_one);
        func_builder.ins().jump(loop_header, &[]);

        // Exit block: return i
        func_builder.switch_to_block(exit_block);
        let return_val = func_builder.use_var(i_var);
        func_builder.ins().return_(&[return_val]);

        // Seal all blocks
        func_builder.seal_block(entry_block);
        func_builder.seal_block(loop_header);
        func_builder.seal_block(loop_body);
        func_builder.seal_block(exit_block);

        func_builder.finalize();

        // Print the CLIF IR
        println!("Function CLIF IR:\n{}", func.display());

        // Declare function
        let func_id = codegen.module.declare_function("main", Linkage::Export, &signature).unwrap();

        // Create context and assign function
        let mut ctx = codegen.module.make_context();
        ctx.func = func;

        // Define the function
        codegen.module.define_function(func_id, &mut ctx).unwrap();

        // Finalize function definitions
        match &mut codegen.module {
            ModuleType::JITModule(jit) => jit.finalize_definitions().unwrap(),
            ModuleType::ObjectModule(_) => panic!("Cannot finalize definitions in object module"),
        }

        // Run main and assert result
        codegen.functions.insert("main".to_string(), func_id);
        let result = codegen.run_main::<i32>().unwrap();
        assert_eq!(result, 10); // The loop will increment i until it equals 10
    }
    
    #[test]
    fn test_pointer() {
        // Equivalent to:
        // int main() { 
        //     int x = 42;
        //     int* ptr = &x;
        //     *ptr = 100;
        //     return x;
        // }
        let mut codegen = get_compiler().unwrap();

        let mut signature = codegen.module.make_signature();
        signature.returns.push(AbiParam::new(types::I32));

        let mut func = Function::new();
        func.signature = signature.clone();

        let mut func_builder_ctx = FunctionBuilderContext::new();
        let mut func_builder = FunctionBuilder::new(&mut func, &mut func_builder_ctx);

        // Create entry block
        let entry_block = func_builder.create_block();
        func_builder.switch_to_block(entry_block);

        // Create stack slot for x
        let stack_slot = func_builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            4,  // size in bytes for i32
            8   // alignment in bytes
        ));

        // Store initial value (42) to stack
        let forty_two = func_builder.ins().iconst(types::I32, 42);
        func_builder.ins().stack_store(forty_two, stack_slot, 0);

        // Load address of stack slot (simulating &x)
        let ptr = func_builder.ins().stack_addr(types::I64, stack_slot, 0);

        // Store new value (100) through the pointer
        let hundred = func_builder.ins().iconst(types::I32, 100);
        func_builder.ins().store(MemFlags::new(), hundred, ptr, 0);

        // Load final value and return
        let result = func_builder.ins().stack_load(types::I32, stack_slot, 0);
        func_builder.ins().return_(&[result]);

        // Seal the block and finalize
        func_builder.seal_block(entry_block);
        func_builder.finalize();

        // Print the CLIF IR
        println!("Function CLIF IR:\n{}", func.display());

        // Declare function
        let func_id = codegen.module.declare_function("main", Linkage::Export, &signature).unwrap();

        // Create context and assign function
        let mut ctx = codegen.module.make_context();
        ctx.func = func;

        // Define the function
        codegen.module.define_function(func_id, &mut ctx).unwrap();

        // Finalize function definitions
        match &mut codegen.module {
            ModuleType::JITModule(jit) => jit.finalize_definitions().unwrap(),
            ModuleType::ObjectModule(_) => panic!("Cannot finalize definitions in object module"),
        }

        // Run main and assert result
        codegen.functions.insert("main".to_string(), func_id);
        let result = codegen.run_main::<i32>().unwrap();
        assert_eq!(result, 100); // Should return 100 after pointer modification
    }
}
