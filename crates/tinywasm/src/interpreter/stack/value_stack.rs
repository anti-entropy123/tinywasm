use alloc::vec::Vec;
use tinywasm_types::{LocalCounts, ValType, WasmValue};

use crate::{interpreter::values::*, Result};

use super::Locals;
pub(crate) const STACK_32_SIZE: usize = 1024 * 128;
pub(crate) const STACK_64_SIZE: usize = 1024 * 128;
pub(crate) const STACK_128_SIZE: usize = 1024 * 128;
pub(crate) const STACK_REF_SIZE: usize = 1024;

#[derive(Debug)]
pub(crate) struct ValueStack {
    pub(crate) stack_32: Vec<Value32>,
    pub(crate) stack_64: Vec<Value64>,
    pub(crate) stack_128: Vec<Value128>,
    pub(crate) stack_ref: Vec<ValueRef>,
}

impl ValueStack {
    pub(crate) fn new() -> Self {
        Self {
            stack_32: Vec::with_capacity(STACK_32_SIZE),
            stack_64: Vec::with_capacity(STACK_64_SIZE),
            stack_128: Vec::with_capacity(STACK_128_SIZE),
            stack_ref: Vec::with_capacity(STACK_REF_SIZE),
        }
    }

    pub(crate) fn height(&self) -> StackLocation {
        StackLocation {
            s32: self.stack_32.len() as u32,
            s64: self.stack_64.len() as u32,
            s128: self.stack_128.len() as u32,
            sref: self.stack_ref.len() as u32,
        }
    }

    #[inline]
    pub(crate) fn peek<T: InternalValue>(&self) -> Result<T> {
        T::stack_peek(self)
    }

    #[inline]
    pub(crate) fn pop<T: InternalValue>(&mut self) -> Result<T> {
        T::stack_pop(self)
    }

    #[inline]
    pub(crate) fn push<T: InternalValue>(&mut self, value: T) {
        T::stack_push(self, value)
    }

    pub(crate) fn drop<T: InternalValue>(&mut self) -> Result<()> {
        T::stack_pop(self).map(|_| ())
    }

    // TODO: this needs to re-introduce the top replacement optimization
    pub(crate) fn select<T: InternalValue>(&mut self) -> Result<()> {
        let cond: i32 = self.pop()?;
        let val2: T = self.pop()?;
        if cond == 0 {
            self.drop::<T>()?;
            self.push(val2);
        }
        Ok(())
    }

    // TODO: this needs to re-introduce the top replacement optimization
    pub(crate) fn calculate<T: InternalValue, U: InternalValue>(&mut self, func: fn(T, T) -> Result<U>) -> Result<()> {
        let v2 = T::stack_pop(self)?;
        let v1 = T::stack_pop(self)?;
        U::stack_push(self, func(v1, v2)?);
        Ok(())
    }

    // TODO: this needs to re-introduce the top replacement optimization
    pub(crate) fn replace_top<T: InternalValue, U: InternalValue>(&mut self, func: fn(T) -> Result<U>) -> Result<()> {
        let v1 = T::stack_pop(self)?;
        U::stack_push(self, func(v1)?);
        Ok(())
    }

    pub(crate) fn pop_dyn(&mut self, val_type: ValType) -> Result<TinyWasmValue> {
        match val_type {
            ValType::I32 => self.pop().map(TinyWasmValue::Value32),
            ValType::I64 => self.pop().map(TinyWasmValue::Value64),
            ValType::V128 => self.pop().map(TinyWasmValue::Value128),
            ValType::RefExtern => self.pop().map(TinyWasmValue::ValueRef),
            ValType::RefFunc => self.pop().map(TinyWasmValue::ValueRef),
            ValType::F32 => self.pop().map(TinyWasmValue::Value32),
            ValType::F64 => self.pop().map(TinyWasmValue::Value64),
        }
    }

    pub(crate) fn pop_params(&mut self, val_types: &[ValType]) -> Result<Vec<WasmValue>> {
        val_types.iter().map(|val_type| self.pop_wasmvalue(*val_type)).collect::<Result<Vec<_>>>()
    }

    pub(crate) fn pop_results(&mut self, val_types: &[ValType]) -> Result<Vec<WasmValue>> {
        val_types.iter().rev().map(|val_type| self.pop_wasmvalue(*val_type)).collect::<Result<Vec<_>>>().map(|mut v| {
            v.reverse();
            v
        })
    }

    // TODO: a lot of optimization potential here
    pub(crate) fn pop_locals(&mut self, val_types: &[ValType], lc: LocalCounts) -> Result<Locals> {
        let mut locals_32 = Vec::new();
        locals_32.reserve_exact(lc.local_32 as usize);
        let mut locals_64 = Vec::new();
        locals_64.reserve_exact(lc.local_64 as usize);
        let mut locals_128 = Vec::new();
        locals_128.reserve_exact(lc.local_128 as usize);
        let mut locals_ref = Vec::new();
        locals_ref.reserve_exact(lc.local_ref as usize);

        for ty in val_types {
            match self.pop_dyn(*ty)? {
                TinyWasmValue::Value32(v) => locals_32.push(v),
                TinyWasmValue::Value64(v) => locals_64.push(v),
                TinyWasmValue::Value128(v) => locals_128.push(v),
                TinyWasmValue::ValueRef(v) => locals_ref.push(v),
            }
        }
        locals_32.reverse();
        locals_32.resize_with(lc.local_32 as usize, Default::default);
        locals_64.reverse();
        locals_64.resize_with(lc.local_64 as usize, Default::default);
        locals_128.reverse();
        locals_128.resize_with(lc.local_128 as usize, Default::default);
        locals_ref.reverse();
        locals_ref.resize_with(lc.local_ref as usize, Default::default);

        Ok(Locals {
            locals_32: locals_32.into_boxed_slice(),
            locals_64: locals_64.into_boxed_slice(),
            locals_128: locals_128.into_boxed_slice(),
            locals_ref: locals_ref.into_boxed_slice(),
        })
    }

    pub(crate) fn truncate_keep(&mut self, to: &StackLocation, keep: &StackHeight) {
        #[inline(always)]
        fn truncate_keep<T: Copy + Default>(data: &mut Vec<T>, n: u32, end_keep: u32) {
            let len = data.len() as u32;
            if len <= n {
                return; // No need to truncate if the current size is already less than or equal to total_to_keep
            }
            data.drain((n as usize)..(len - end_keep) as usize);
        }

        truncate_keep(&mut self.stack_32, to.s32, keep.s32 as u32);
        truncate_keep(&mut self.stack_64, to.s64, keep.s64 as u32);
        truncate_keep(&mut self.stack_128, to.s128, keep.s128 as u32);
        truncate_keep(&mut self.stack_ref, to.sref, keep.sref as u32);
    }

    pub(crate) fn push_dyn(&mut self, value: TinyWasmValue) {
        match value {
            TinyWasmValue::Value32(v) => self.stack_32.push(v),
            TinyWasmValue::Value64(v) => self.stack_64.push(v),
            TinyWasmValue::Value128(v) => self.stack_128.push(v),
            TinyWasmValue::ValueRef(v) => self.stack_ref.push(v),
        }
    }

    pub(crate) fn pop_wasmvalue(&mut self, val_type: ValType) -> Result<WasmValue> {
        match val_type {
            ValType::I32 => self.pop().map(WasmValue::I32),
            ValType::I64 => self.pop().map(WasmValue::I64),
            ValType::V128 => self.pop().map(WasmValue::V128),
            ValType::F32 => self.pop().map(WasmValue::F32),
            ValType::F64 => self.pop().map(WasmValue::F64),
            ValType::RefExtern => self.pop().map(|v| match v {
                Some(v) => WasmValue::RefExtern(v),
                None => WasmValue::RefNull(ValType::RefExtern),
            }),
            ValType::RefFunc => self.pop().map(|v| match v {
                Some(v) => WasmValue::RefFunc(v),
                None => WasmValue::RefNull(ValType::RefFunc),
            }),
        }
    }

    pub(crate) fn extend_from_wasmvalues(&mut self, values: &[WasmValue]) {
        for value in values.iter() {
            self.push_dyn(value.into())
        }
    }
}
