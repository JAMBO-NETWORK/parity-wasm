use std::sync::Arc;
use std::collections::HashMap;
use std::borrow::Cow;
use parking_lot::RwLock;
use elements::{FunctionType, Internal, ValueType};
use interpreter::Error;
use interpreter::module::{ModuleInstanceInterface, ExecutionParams, ItemIndex,
	CallerContext, ExportEntryType};
use interpreter::memory::MemoryInstance;
use interpreter::table::TableInstance;
use interpreter::value::RuntimeValue;
use interpreter::variable::{VariableInstance, VariableType};

/// Min index of native function.
pub const NATIVE_INDEX_FUNC_MIN: u32 = 10001;

/// User function closure type.
// pub type UserFunctionClosure<'a> = &'a mut FnMut(context: CallerContext) -> Result<Option<RuntimeValue>, Error>;

/// User functions executor.
pub trait UserFunctionExecutor {
	/// Execute function with given name.
	fn execute(&mut self, name: &str, context: CallerContext) -> Result<Option<RuntimeValue>, Error>;
}

#[derive(Clone)]
pub enum UserFunctionDescriptor {
	Static(&'static str, &'static [ValueType]),
	Heap(String, Vec<ValueType>),
}

/// User function type.
#[derive(Clone)]
pub struct UserFunction {
	pub desc: UserFunctionDescriptor,
	pub result: Option<ValueType>,
}

impl UserFunction {
	/// New function with statically known params
	pub fn statik(name: &'static str, params: &'static [ValueType], result: Option<ValueType>) -> Self {
		UserFunction {
			desc: UserFunctionDescriptor::Static(name, params),
			result: result,
		}
	}

	/// New function with statically unknown params
	pub fn heap(name: String, params: Vec<ValueType>, result: Option<ValueType>) -> Self {
		UserFunction {
			desc: UserFunctionDescriptor::Heap(name, params),
			result: result,
		}	
	}

	/// Name of the function
	pub fn name(&self) -> &str {
		match self.desc {
			UserFunctionDescriptor::Static(name, _) => name,
			UserFunctionDescriptor::Heap(ref name, _) => name,
		}
	}

	/// Arguments of the function
	pub fn params(&self) -> &[ValueType] {
		match self.desc {
			UserFunctionDescriptor::Static(_, params) => params,
			UserFunctionDescriptor::Heap(_, ref params) => params,
		}		
	}

	/// Return type of the function
	pub fn result(&self) -> Option<ValueType> {
		self.result
	}
}

/// Set of user-defined functions
pub struct UserFunctions<'a> {
	/// Functions list.
	pub functions: Cow<'static, [UserFunction]>,
	/// Functions executor.
	pub executor: &'a mut UserFunctionExecutor,
}

/// Native module instance.
pub struct NativeModuleInstance<'a> {
	/// Underllying module reference.
	env: Arc<ModuleInstanceInterface>,
	/// User function executor.
	executor: RwLock<&'a mut UserFunctionExecutor>,
	/// By-name functions index.
	by_name: HashMap<String, u32>,
	/// User functions list.
	functions: Cow<'static, [UserFunction]>,
}

impl<'a> NativeModuleInstance<'a> {
	/// Create new native module
	pub fn new(env: Arc<ModuleInstanceInterface>, functions: UserFunctions<'a>) -> Result<Self, Error> {
		Ok(NativeModuleInstance {
			env: env,
			executor: RwLock::new(functions.executor),
			by_name: functions.functions.iter().enumerate().map(|(i, f)| (f.name().to_owned(), i as u32)).collect(),
			functions: functions.functions,
		})
	}
}

impl<'a> ModuleInstanceInterface for NativeModuleInstance<'a> {
	fn instantiate<'b>(&self, is_user_module: bool, externals: Option<&'b HashMap<String, Arc<ModuleInstanceInterface + 'b>>>) -> Result<(), Error> {
		self.env.instantiate(is_user_module, externals)
	}

	fn execute_index(&self, index: u32, params: ExecutionParams) -> Result<Option<RuntimeValue>, Error> {
		self.env.execute_index(index, params)
	}

	fn execute_export(&self, name: &str, params: ExecutionParams) -> Result<Option<RuntimeValue>, Error> {
		self.env.execute_export(name, params)
	}

	fn export_entry<'b>(&self, name: &str, externals: Option<&'b HashMap<String, Arc<ModuleInstanceInterface + 'b>>>, required_type: &ExportEntryType) -> Result<Internal, Error> {
		if let Some(index) = self.by_name.get(name) {
			return Ok(Internal::Function(NATIVE_INDEX_FUNC_MIN + *index));
		}

		self.env.export_entry(name, externals, required_type)
	}

	fn function_type<'b>(&self, function_index: ItemIndex, externals: Option<&'b HashMap<String, Arc<ModuleInstanceInterface + 'b>>>) -> Result<FunctionType, Error> {
		let index = match function_index {
			ItemIndex::IndexSpace(index) | ItemIndex::Internal(index) => index,
			ItemIndex::External(_) => unreachable!("trying to call function, exported by native env module"),
		};

		if index < NATIVE_INDEX_FUNC_MIN {
			return self.env.function_type(function_index, externals);
		}

		self.functions
			.get((index - NATIVE_INDEX_FUNC_MIN) as usize)
			.ok_or(Error::Native(format!("missing native env function with index {}", index)))
			.map(|f| FunctionType::new(f.params().to_vec(), f.result().clone()))
	}

	fn table(&self, index: ItemIndex) -> Result<Arc<TableInstance>, Error> {
		self.env.table(index)
	}

	fn memory(&self, index: ItemIndex) -> Result<Arc<MemoryInstance>, Error> {
		self.env.memory(index)
	}

	fn global(&self, index: ItemIndex, variable_type: Option<VariableType>) -> Result<Arc<VariableInstance>, Error> {
		self.env.global(index, variable_type)
	}

	fn call_function(&self, outer: CallerContext, index: ItemIndex, function_type: Option<&FunctionType>) -> Result<Option<RuntimeValue>, Error> {
		self.env.call_function(outer, index, function_type)
	}

	fn call_function_indirect(&self, outer: CallerContext, table_index: ItemIndex, type_index: u32, func_index: u32) -> Result<Option<RuntimeValue>, Error> {
		self.env.call_function_indirect(outer, table_index, type_index, func_index)
	}

	fn call_internal_function(&self, outer: CallerContext, index: u32, function_type: Option<&FunctionType>) -> Result<Option<RuntimeValue>, Error> {
		if index < NATIVE_INDEX_FUNC_MIN {
			return self.env.call_internal_function(outer, index, function_type);
		}

		// TODO: check type
		self.functions
			.get((index - NATIVE_INDEX_FUNC_MIN) as usize)
			.ok_or(Error::Native(format!("trying to call native function with index {}", index)))
			.and_then(|f| self.executor.write().execute(&f.name(), outer))
	}
}

/// Create wrapper for env module with given native user functions.
pub fn env_native_module(env: Arc<ModuleInstanceInterface>, user_functions: UserFunctions) -> Result<NativeModuleInstance, Error> {
	NativeModuleInstance::new(env, user_functions)
}
