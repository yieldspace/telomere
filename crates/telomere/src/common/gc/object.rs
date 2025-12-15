mod root_table;
pub(crate) use root_table::RootTable;
mod ref_dynamic_array;
pub(crate) use ref_dynamic_array::GcRefDynamicArray;
mod ref_fixed_array;
pub(crate) use ref_fixed_array::GcRefFixedArray;
#[cfg(test)]
mod u32_fixed_array;
#[cfg(test)]
pub(crate) use u32_fixed_array::U32FixedArray;
mod instance_data;
pub(crate) use instance_data::InstanceData;
mod global_data;
pub(crate) use global_data::Global16Data;
pub(crate) use global_data::Global4Data;
pub(crate) use global_data::Global8Data;
pub(crate) use global_data::GlobalRefData;
mod function_instance_data;
pub(crate) use function_instance_data::FunctionInstanceData;
