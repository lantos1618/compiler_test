use anyhow::Result;
use control::ControlPlane;
use cranelift::prelude::*;
use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::Constant;
use cranelift_codegen::ir::ExternalName;
use cranelift_codegen::*;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{
    DataDescription, DataId, FuncId, FuncOrDataId, Linkage, Module, ModuleDeclarations,
    ModuleResult,
};
use cranelift_object::ObjectModule;
use delegate::delegate;
use ir::{FuncRef, Function};
use isa::{TargetFrontendConfig, TargetIsa};
use std::collections::HashMap;
use target_lexicon;
use crate::codegen::ir::immediates::Offset32;

pub struct Codegen {
    pub module: ModuleType,
    pub functions: HashMap<String, FuncId>,
}

pub enum ModuleType {
    JITModule(JITModule),
    ObjectModule(ObjectModule),
}

// this is a helper to delegate the methods to the correct underlying method
impl ModuleType {
    delegate! {
        to match self {
            Self::JITModule(jit) => jit,
            Self::ObjectModule(obj) => obj,
        } {
            fn isa(&self) -> &dyn TargetIsa;
            fn declarations(&self) -> &ModuleDeclarations;
            fn declare_function(&mut self, name: &str, linkage: Linkage, signature: &Signature) -> ModuleResult<FuncId>;
            fn declare_func_in_func(&mut self, func_id: FuncId, func: &mut Function) -> FuncRef;
            fn declare_anonymous_function(&mut self, signature: &Signature) -> ModuleResult<FuncId>;
            fn declare_data(&mut self, name: &str, linkage: Linkage, writable: bool, tls: bool) -> ModuleResult<DataId>;
            fn declare_anonymous_data(&mut self, writable: bool, tls: bool) -> ModuleResult<DataId>;
            fn define_function(&mut self, func_id: FuncId, ctx: &mut Context) -> ModuleResult<()>;
            fn define_function_with_control_plane(&mut self, func_id: FuncId, ctx: &mut Context, ctrl_plane: &mut ControlPlane) -> ModuleResult<()>;
            fn define_function_bytes(&mut self, func_id: FuncId, func: &Function, alignment: u64, bytes: &[u8], relocs: &[FinalizedMachReloc]) -> ModuleResult<()>;
            fn define_data(&mut self, data_id: DataId, data: &DataDescription) -> ModuleResult<()>;
            fn get_name(&self, name: &str) -> Option<FuncOrDataId>;
            fn make_signature(&self) -> Signature;
            fn make_context(&self) -> Context;
            fn clear_context(&self, ctx: &mut Context);
            fn clear_signature(&self, sig: &mut Signature);
            fn target_config(&self) -> TargetFrontendConfig;
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
            ModuleType::ObjectModule(_) => {
                return Err(anyhow::anyhow!("Cannot run functions from object module"))
            }
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
        func.signature = signature.clone(); // <-- Add this line

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
        let func_id = codegen
            .module
            .declare_function("main", Linkage::Export, &signature)
            .unwrap();

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
        func_builder
            .ins()
            .brif(zero, true_block, &[], false_block, &[]);

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
        let func_id = codegen
            .module
            .declare_function("main", Linkage::Export, &signature)
            .unwrap();

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
        func_builder
            .ins()
            .brif(condition, loop_body, &[], exit_block, &[]);

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
        let func_id = codegen
            .module
            .declare_function("main", Linkage::Export, &signature)
            .unwrap();

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
    fn test_function_definition_and_call() {
        // int add(int a, int b) { return a + b; }
        // int main() { return add(1, 2); }

        let mut codegen = get_compiler().unwrap();

        // Define signature for add function (takes two i32, returns i32)
        let mut add_signature = codegen.module.make_signature();
        add_signature.params.push(AbiParam::new(types::I32));
        add_signature.params.push(AbiParam::new(types::I32));
        add_signature.returns.push(AbiParam::new(types::I32));

        // Create add function
        let mut add_func = Function::new();
        add_func.signature = add_signature.clone();

        let mut add_func_builder_ctx = FunctionBuilderContext::new();
        let mut add_func_builder = FunctionBuilder::new(&mut add_func, &mut add_func_builder_ctx);

        // Create entry block for add function
        let add_entry_block = add_func_builder.create_block();
        add_func_builder.switch_to_block(add_entry_block);

        // Get the parameters a and b
        let a = add_func_builder.append_block_param(add_entry_block, types::I32);
        let b = add_func_builder.append_block_param(add_entry_block, types::I32);

        // Perform the addition: a + b
        let sum = add_func_builder.ins().iadd(a, b);
        add_func_builder.ins().return_(&[sum]);

        add_func_builder.seal_block(add_entry_block);
        add_func_builder.finalize();

        // Declare add function
        let add_func_id = codegen
            .module
            .declare_function("add", Linkage::Export, &add_signature)
            .unwrap();

        // Create context and assign add function
        let mut add_ctx = codegen.module.make_context();
        add_ctx.func = add_func;

        // Define add function
        codegen
            .module
            .define_function(add_func_id, &mut add_ctx)
            .unwrap();

        // Now, create the main function that calls add(1, 2)

        let mut main_signature = codegen.module.make_signature();
        main_signature.returns.push(AbiParam::new(types::I32));

        let mut main_func = Function::new();
        main_func.signature = main_signature.clone();

        let mut main_func_builder_ctx = FunctionBuilderContext::new();
        let mut main_func_builder =
            FunctionBuilder::new(&mut main_func, &mut main_func_builder_ctx);

        // Create entry block for main function
        let main_entry_block = main_func_builder.create_block();
        main_func_builder.switch_to_block(main_entry_block);

        // Define the arguments to pass to add (1, 2)
        let one = main_func_builder.ins().iconst(types::I32, 1);
        let two = main_func_builder.ins().iconst(types::I32, 2);

        // Convert FuncId to FuncRef
        let add_func_ref = codegen
            .module
            .declare_func_in_func(add_func_id, &mut main_func_builder.func);

        // Call the add function
        let call = main_func_builder.ins().call(add_func_ref, &[one, two]);
        let result = main_func_builder.inst_results(call)[0];

        // Return the result of add(1, 2)
        main_func_builder.ins().return_(&[result]);

        main_func_builder.seal_block(main_entry_block);
        main_func_builder.finalize();

        // Declare main function
        let main_func_id = codegen
            .module
            .declare_function("main", Linkage::Export, &main_signature)
            .unwrap();

        // Create context and assign main function
        let mut main_ctx = codegen.module.make_context();
        main_ctx.func = main_func;

        // Define main function

        codegen
            .module
            .define_function(main_func_id, &mut main_ctx)
            .unwrap();

        // Finalize function definitions
        match &mut codegen.module {
            ModuleType::JITModule(jit) => jit.finalize_definitions().unwrap(),
            ModuleType::ObjectModule(_) => panic!("Cannot finalize definitions in object module"),
        }

        // Insert main function into codegen and run it
        codegen.functions.insert("main".to_string(), main_func_id);
        let result = codegen.run_main::<i32>().unwrap();

        // Assert that main returns the expected result (1 + 2 = 3)
        assert_eq!(result, 3);
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
            4, // size in bytes for i32
            8, // alignment in bytes
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
        let func_id = codegen
            .module
            .declare_function("main", Linkage::Export, &signature)
            .unwrap();

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

    #[test]
    fn test_arithmetic_operations() {
        // Equivalent to:
        // int main() {
        //     int a = 10;
        //     int b = 3;
        //     int sum = a + b;        // 13
        //     int diff = a - b;       // 7
        //     int prod = a * b;       // 30
        //     int quot = a / b;       // 3
        //     int rem = a % b;        // 1
        //     return sum + diff + prod + quot + rem; // 54
        // }
        let mut codegen = get_compiler().unwrap();

        let mut signature = codegen.module.make_signature();
        signature.returns.push(AbiParam::new(types::I32));

        let mut func = Function::new();
        func.signature = signature.clone();

        let mut func_builder_ctx = FunctionBuilderContext::new();
        let mut func_builder = FunctionBuilder::new(&mut func, &mut func_builder_ctx);

        let entry_block = func_builder.create_block();
        func_builder.switch_to_block(entry_block);

        // Create the values
        let a = func_builder.ins().iconst(types::I32, 10);
        let b = func_builder.ins().iconst(types::I32, 3);

        // Perform arithmetic operations
        let sum = func_builder.ins().iadd(a, b);
        let diff = func_builder.ins().isub(a, b);
        let prod = func_builder.ins().imul(a, b);
        let quot = func_builder.ins().sdiv(a, b);
        let rem = func_builder.ins().srem(a, b);

        // Sum all results
        let temp1 = func_builder.ins().iadd(sum, diff);
        let temp2 = func_builder.ins().iadd(temp1, prod);
        let temp3 = func_builder.ins().iadd(temp2, quot);
        let final_result = func_builder.ins().iadd(temp3, rem);

        func_builder.ins().return_(&[final_result]);

        func_builder.seal_block(entry_block);
        func_builder.finalize();

        // Declare and define function
        let func_id = codegen
            .module
            .declare_function("main", Linkage::Export, &signature)
            .unwrap();
        let mut ctx = codegen.module.make_context();
        ctx.func = func;
        codegen.module.define_function(func_id, &mut ctx).unwrap();

        // Finalize function definitions
        match &mut codegen.module {
            ModuleType::JITModule(jit) => jit.finalize_definitions().unwrap(),
            ModuleType::ObjectModule(_) => panic!("Cannot finalize definitions in object module"),
        }

        // Run main and assert result
        codegen.functions.insert("main".to_string(), func_id);
        let result = codegen.run_main::<i32>().unwrap();
        assert_eq!(result, 54); // 13 + 7 + 30 + 3 + 1 = 54
    }

    #[test]
    fn test_simd() {
        // Get compiler instance
        let mut codegen = get_compiler().unwrap();

        let mut signature = codegen.module.make_signature();
        signature.returns.push(AbiParam::new(types::I32));

        let mut func = Function::new();
        func.signature = signature.clone();

        let mut func_builder_ctx = FunctionBuilderContext::new();
        let mut func_builder = FunctionBuilder::new(&mut func, &mut func_builder_ctx);

        let entry_block = func_builder.create_block();
        func_builder.switch_to_block(entry_block);

        // Create SIMD vector type I32x4 (vector of 4 32-bit integers)
        let i32x4_type = types::I32X4;

        // Create first vector [1,2,3,4]
        let a_const = func_builder.ins().iconst(types::I32, 1);
        let b_const = func_builder.ins().iconst(types::I32, 2);
        let c_const = func_builder.ins().iconst(types::I32, 3);
        let d_const = func_builder.ins().iconst(types::I32, 4);

        // Create second vector [10,20,30,40]
        let e_const = func_builder.ins().iconst(types::I32, 10);
        let f_const = func_builder.ins().iconst(types::I32, 20);
        let g_const = func_builder.ins().iconst(types::I32, 30);
        let h_const = func_builder.ins().iconst(types::I32, 40);

        // Create SIMD vectors
        let a_vector = func_builder.ins().scalar_to_vector(i32x4_type, a_const);
        let a_vector = func_builder.ins().insertlane(a_vector, b_const, 1);
        let a_vector = func_builder.ins().insertlane(a_vector, c_const, 2);
        let a_vector = func_builder.ins().insertlane(a_vector, d_const, 3);

        let b_vector = func_builder.ins().scalar_to_vector(i32x4_type, e_const);
        let b_vector = func_builder.ins().insertlane(b_vector, f_const, 1);
        let b_vector = func_builder.ins().insertlane(b_vector, g_const, 2);
        let b_vector = func_builder.ins().insertlane(b_vector, h_const, 3);

        // Add vectors
        let result_vector = func_builder.ins().iadd(a_vector, b_vector);

        // Extract first lane (should be 11 = 1 + 10)
        let result = func_builder.ins().extractlane(result_vector, 0);

        // Return the result
        func_builder.ins().return_(&[result]);

        func_builder.seal_block(entry_block);
        func_builder.finalize();

        // Declare function
        let func_id = codegen
            .module
            .declare_function("main", Linkage::Export, &signature)
            .unwrap();

        // print the CLIF IR
        println!("Function CLIF IR:\n{}", func.display());

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
        assert_eq!(result, 11); // First element should be 1 + 10 = 11
    }

    #[test]
    fn test_struct() {
        // Struct Point {
        //  x: i32
        //  y: i32
        // }
        // int main() {
        //     Point p;
        //     p.x = 1;
        //     p.y = 2;
        //     return p.x + p.y;
        // }
        // Create compiler instance
        let mut codegen = get_compiler().unwrap();

        let mut signature = codegen.module.make_signature();
        signature.returns.push(AbiParam::new(types::I32));

        let mut func = Function::new();
        func.signature = signature.clone();

        let mut func_builder_ctx = FunctionBuilderContext::new();
        let mut func_builder = FunctionBuilder::new(&mut func, &mut func_builder_ctx);

        let entry_block = func_builder.create_block();
        func_builder.switch_to_block(entry_block);

        // Create a stack slot for our Point struct (8 bytes: 4 for x, 4 for y)
        let point_struct = func_builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            8, // size in bytes (4 bytes each for x and y)
            4, // alignment
        ));

        // Store x = 1 at offset 0
        let x_value = func_builder.ins().iconst(types::I32, 1);
        func_builder.ins().stack_store(x_value, point_struct, 0);

        // Store y = 2 at offset 4
        let y_value = func_builder.ins().iconst(types::I32, 2);
        func_builder.ins().stack_store(y_value, point_struct, 4);

        // Load x and y back from memory
        let loaded_x = func_builder.ins().stack_load(types::I32, point_struct, 0);
        let loaded_y = func_builder.ins().stack_load(types::I32, point_struct, 4);

        // Add x + y
        let result = func_builder.ins().iadd(loaded_x, loaded_y);

        // Return the result
        func_builder.ins().return_(&[result]);

        func_builder.seal_block(entry_block);
        func_builder.finalize();

        // Print the CLIF IR
        println!("Function CLIF IR:\n{}", func.display());

        // Declare function
        let func_id = codegen
            .module
            .declare_function("main", Linkage::Export, &signature)
            .unwrap();

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
        assert_eq!(result, 3); // Should return 1 + 2 = 3
    }

    #[test]
    fn test_complex_struct() {
        // struct Point {
        //     x: i32
        //     y: i32
        // }
        // struct Line {
        //     start: Point
        //     end: Point
        // }
        // int main() {
        //     Line line;
        //     line.start.x = 1;
        //     line.start.y = 2;
        //     line.end.x = 3;
        //     line.end.y = 4;
        //     return line.start.x + line.start.y + line.end.x + line.end.y;
        // }

        let mut codegen = get_compiler().unwrap();

        let mut signature = codegen.module.make_signature();
        signature.returns.push(AbiParam::new(types::I32));

        let mut func = Function::new();
        func.signature = signature.clone();

        let mut func_builder_ctx = FunctionBuilderContext::new();
        let mut func_builder = FunctionBuilder::new(&mut func, &mut func_builder_ctx);

        let entry_block = func_builder.create_block();
        func_builder.switch_to_block(entry_block);

        // Create a stack slot for Line struct
        // Size = 16 bytes (2 Points  (2 i32s × 4 bytes))
        // Layout:
        // 0-3:   start.x
        // 4-7:   start.y
        // 8-11:  end.x
        // 12-15: end.y
        let line_struct = func_builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            16, // total size in bytes
            4,  // alignment
        ));

        // Store line.start.x = 1 at offset 0
        let start_x = func_builder.ins().iconst(types::I32, 1);
        func_builder.ins().stack_store(start_x, line_struct, 0);

        // Store line.start.y = 2 at offset 4
        let start_y = func_builder.ins().iconst(types::I32, 2);
        func_builder.ins().stack_store(start_y, line_struct, 4);

        // Store line.end.x = 3 at offset 8
        let end_x = func_builder.ins().iconst(types::I32, 3);
        func_builder.ins().stack_store(end_x, line_struct, 8);

        // Store line.end.y = 4 at offset 12
        let end_y = func_builder.ins().iconst(types::I32, 4);
        func_builder.ins().stack_store(end_y, line_struct, 12);

        // Load all values back from memory
        let loaded_start_x = func_builder.ins().stack_load(types::I32, line_struct, 0);
        let loaded_start_y = func_builder.ins().stack_load(types::I32, line_struct, 4);
        let loaded_end_x = func_builder.ins().stack_load(types::I32, line_struct, 8);
        let loaded_end_y = func_builder.ins().stack_load(types::I32, line_struct, 12);

        // Add all values: start.x + start.y + end.x + end.y
        let sum1 = func_builder.ins().iadd(loaded_start_x, loaded_start_y);
        let sum2 = func_builder.ins().iadd(loaded_end_x, loaded_end_y);
        let result = func_builder.ins().iadd(sum1, sum2);

        // Return the result
        func_builder.ins().return_(&[result]);

        func_builder.seal_block(entry_block);
        func_builder.finalize();

        // Print the CLIF IR
        println!("Function CLIF IR:\n{}", func.display());

        // Declare function
        let func_id = codegen
            .module
            .declare_function("main", Linkage::Export, &signature)
            .unwrap();

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
        assert_eq!(result, 10); // Should return 1 + 2 + 3 + 4 = 10
    }
    #[test]
    fn test_vector() {
        // Simulating:
        // int main() {
        //     int arr[4];  // Stack-allocated array
        //     arr[0] = 1;
        //     arr[1] = 2;
        //     arr[2] = 3;
        //     arr[3] = 4;
        //     return arr[0] + arr[1] + arr[2] + arr[3];
        // }

        let mut codegen = get_compiler().unwrap();

        let mut signature = codegen.module.make_signature();
        signature.returns.push(AbiParam::new(types::I32));

        let mut func = Function::new();
        func.signature = signature.clone();

        let mut func_builder_ctx = FunctionBuilderContext::new();
        let mut func_builder = FunctionBuilder::new(&mut func, &mut func_builder_ctx);

        let entry_block = func_builder.create_block();
        func_builder.switch_to_block(entry_block);

        // Create a stack slot for our array (16 bytes: 4 integers × 4 bytes each)
        let array = func_builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            16, // size in bytes (4 integers × 4 bytes)
            4,  // alignment
        ));

        // Get base address of array
        let base_addr = func_builder.ins().stack_addr(types::I64, array, 0);

        // Store values in array
        for i in 0..4 {
            let value = func_builder.ins().iconst(types::I32, (i + 1) as i64);
            let offset = (i * 4) as i32; // Each integer is 4 bytes
            func_builder
                .ins()
                .store(MemFlags::new(), value, base_addr, offset);
        }

        // Load values back and sum them
        let mut sum = func_builder.ins().iconst(types::I32, 0);
        for i in 0..4 {
            let offset = (i * 4) as i32;
            let loaded_value =
                func_builder
                    .ins()
                    .load(types::I32, MemFlags::new(), base_addr, offset);
            sum = func_builder.ins().iadd(sum, loaded_value);
        }

        // Return the sum
        func_builder.ins().return_(&[sum]);

        func_builder.seal_block(entry_block);
        func_builder.finalize();

        // Print the CLIF IR
        println!("Function CLIF IR:\n{}", func.display());

        // Declare function
        let func_id = codegen
            .module
            .declare_function("main", Linkage::Export, &signature)
            .unwrap();

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
        assert_eq!(result, 10); // Should return 1 + 2 + 3 + 4 = 10
    }

    #[test]
    fn test_heap_allocation() {
        // Simulating:
        // int main() {
        //     int* ptr = (int*)malloc(sizeof(int) * 4);
        //     ptr[0] = 1;
        //     ptr[1] = 2;
        //     ptr[2] = 3;
        //     ptr[3] = 4;
        //     int sum = ptr[0] + ptr[1] + ptr[2] + ptr[3];
        //     free(ptr);
        //     return sum;
        // }

        let mut codegen = get_compiler().unwrap();

        let mut signature = codegen.module.make_signature();
        signature.returns.push(AbiParam::new(types::I32));

        let mut func = Function::new();
        func.signature = signature.clone();

        let mut func_builder_ctx = FunctionBuilderContext::new();
        let mut func_builder = FunctionBuilder::new(&mut func, &mut func_builder_ctx);

        let entry_block = func_builder.create_block();
        func_builder.switch_to_block(entry_block);

        // Create malloc signature (size_t -> void*)
        let mut malloc_sig = codegen.module.make_signature();
        malloc_sig.params.push(AbiParam::new(types::I64)); // size parameter
        malloc_sig.returns.push(AbiParam::new(types::I64)); // pointer return

        // Create free signature (void* -> void)
        let mut free_sig = codegen.module.make_signature();
        free_sig.params.push(AbiParam::new(types::I64)); // pointer parameter

        // Declare malloc and free functions
        let malloc_func_id = codegen
            .module
            .declare_function("malloc", Linkage::Import, &malloc_sig)
            .unwrap();

        let free_func_id = codegen
            .module
            .declare_function("free", Linkage::Import, &free_sig)
            .unwrap();

        // Get function references
        let malloc_ref = codegen
            .module
            .declare_func_in_func(malloc_func_id, &mut func_builder.func);
        let free_ref = codegen
            .module
            .declare_func_in_func(free_func_id, &mut func_builder.func);

        // Call malloc(sizeof(int) * 4)
        let size = func_builder.ins().iconst(types::I64, 16); // 4 ints * 4 bytes
        let call_malloc = func_builder.ins().call(malloc_ref, &[size]);
        let heap_ptr = func_builder.inst_results(call_malloc)[0];

        // Store values in heap memory
        for i in 0..4 {
            let value = func_builder.ins().iconst(types::I32, (i + 1) as i64);
            let offset = (i * 4) as i32; // Each integer is 4 bytes
            func_builder
                .ins()
                .store(MemFlags::new(), value, heap_ptr, offset);
        }

        // Load and sum values
        let mut sum = func_builder.ins().iconst(types::I32, 0);
        for i in 0..4 {
            let offset = (i * 4) as i32;
            let loaded_value =
                func_builder
                    .ins()
                    .load(types::I32, MemFlags::new(), heap_ptr, offset);
            sum = func_builder.ins().iadd(sum, loaded_value);
        }

        // Call free(ptr)
        func_builder.ins().call(free_ref, &[heap_ptr]);

        // Return the sum
        func_builder.ins().return_(&[sum]);

        func_builder.seal_block(entry_block);
        func_builder.finalize();

        // Print the CLIF IR
        println!("Function CLIF IR:\n{}", func.display());

        // Declare function
        let func_id = codegen
            .module
            .declare_function("main", Linkage::Export, &signature)
            .unwrap();

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
        assert_eq!(result, 10); // Should return 1 + 2 + 3 + 4 = 10
    }

    #[test]
    fn test_printf_call() {
        // Initialize the compiler
        let mut codegen = get_compiler().unwrap();

        // Define the signature for `printf`
        let mut printf_sig = codegen.module.make_signature();
        printf_sig.params.push(AbiParam::new(types::I64));  // pointer to format string
        printf_sig.params.push(AbiParam::new(types::I32));  // integer argument
        printf_sig.returns.push(AbiParam::new(types::I32)); // return value

        // Declare `printf` as an imported function
        let printf_func_id = codegen
            .module
            .declare_function("printf", Linkage::Import, &printf_sig)
            .unwrap();

        // Define the main function signature
        let mut main_signature = codegen.module.make_signature();
        main_signature.returns.push(AbiParam::new(types::I32));

        let mut func = Function::new();
        func.signature = main_signature.clone();

        let mut func_builder_ctx = FunctionBuilderContext::new();
        let mut func_builder = FunctionBuilder::new(&mut func, &mut func_builder_ctx);

        let entry_block = func_builder.create_block();
        func_builder.switch_to_block(entry_block);

        // Create a stack slot for the format string with proper alignment
        let format_str = "Hello, %d!\n\0";
        let stack_slot = func_builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            format_str.len() as u32,
            8,  // Use 8-byte alignment for better compatibility
        ));

        // Get address of the stack slot
        let format_ptr = func_builder.ins().stack_addr(types::I64, stack_slot, 0);

        // Store the format string bytes
        for (i, &byte) in format_str.as_bytes().iter().enumerate() {
            let byte_val = func_builder.ins().iconst(types::I8, byte as i64);
            func_builder.ins().store(
                MemFlags::new(),
                byte_val,
                format_ptr,
                i as i32,
            );
        }

        // Prepare the integer argument for `printf`
        let arg = func_builder.ins().iconst(types::I32, 42);

        // Get printf function reference and call it
        let printf_ref = codegen.module.declare_func_in_func(printf_func_id, &mut func_builder.func);
        let call = func_builder.ins().call(printf_ref, &[format_ptr, arg]);
        
        // Store the printf return value but don't use it
        let _printf_ret = func_builder.inst_results(call)[0];

        // Return 0
        let zero = func_builder.ins().iconst(types::I32, 0);
        func_builder.ins().return_(&[zero]);

        func_builder.seal_block(entry_block);
        func_builder.finalize();

        // Declare and define the main function
        let func_id = codegen
            .module
            .declare_function("main", Linkage::Export, &main_signature)
            .unwrap();

        let mut ctx = codegen.module.make_context();
        ctx.func = func;

        codegen.module.define_function(func_id, &mut ctx).unwrap();

        // Finalize definitions
        match &mut codegen.module {
            ModuleType::JITModule(jit) => jit.finalize_definitions().unwrap(),
            ModuleType::ObjectModule(_) => panic!("Cannot finalize definitions in object module"),
        }

        // Run main and assert result
        codegen.functions.insert("main".to_string(), func_id);
        let result = codegen.run_main::<i32>().unwrap();
        assert_eq!(result, 0);
    }

}
