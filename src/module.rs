use control::ControlPlane;
use cranelift::prelude::*;
use cranelift_codegen::*;
use cranelift_jit::JITModule;
use cranelift_object::ObjectModule;
use cranelift_module::{
    DataDescription, DataId, FuncId, FuncOrDataId, Linkage, Module, ModuleDeclarations,
    ModuleResult,
};
use delegate::delegate;
use ir::{FuncRef, Function, GlobalValue};
use isa::{TargetFrontendConfig, TargetIsa};


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
            pub fn isa(&self) -> &dyn TargetIsa;
            pub fn declarations(&self) -> &ModuleDeclarations;
            pub fn declare_function(&mut self, name: &str, linkage: Linkage, signature: &Signature) -> ModuleResult<FuncId>;
            pub fn declare_func_in_func(&mut self, func_id: FuncId, func: &mut Function) -> FuncRef;
            pub fn declare_anonymous_function(&mut self, signature: &Signature) -> ModuleResult<FuncId>;
            pub fn declare_data(&mut self, name: &str, linkage: Linkage, writable: bool, tls: bool) -> ModuleResult<DataId>;
            pub fn declare_anonymous_data(&mut self, writable: bool, tls: bool) -> ModuleResult<DataId>;
            pub fn declare_data_in_func(&mut self, data_id: DataId, func: &mut Function) -> GlobalValue;
            pub fn define_function(&mut self, func_id: FuncId, ctx: &mut Context) -> ModuleResult<()>;
            pub fn define_function_with_control_plane(&mut self, func_id: FuncId, ctx: &mut Context, ctrl_plane: &mut ControlPlane) -> ModuleResult<()>;
            pub fn define_function_bytes(&mut self, func_id: FuncId, func: &Function, alignment: u64, bytes: &[u8], relocs: &[FinalizedMachReloc]) -> ModuleResult<()>;
            pub fn define_data(&mut self, data_id: DataId, data: &DataDescription) -> ModuleResult<()>;
            pub fn get_name(&self, name: &str) -> Option<FuncOrDataId>;
            pub fn make_signature(&self) -> Signature;
            pub fn make_context(&self) -> Context;
            pub fn clear_context(&self, ctx: &mut Context);
            pub fn clear_signature(&self, sig: &mut Signature);
            pub fn target_config(&self) -> TargetFrontendConfig;
        }
    }
}

