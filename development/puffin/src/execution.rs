use paste::paste;
use std::{mem, ptr};
use std::cmp::max;
use std::mem::MaybeUninit;
use std::ops::{Deref, Range};
use std::process::id;
use std::ptr::{null, NonNull};
use std::sync::Arc;
use enum_assoc::Assoc;
use num_derive::{FromPrimitive, ToPrimitive};
use crate::stack_pool::{Pool, Stack};

use anyhow::{anyhow, Result};
use num_traits::FromPrimitive;
use crate::instruction::Instruction;
use crate::uninit_arr;
// enum Function {
//     Native(param, fn()),
//     Bytecode(usize)
// }


pub(crate) enum ClassMeta {
    Tuple,
    Structured,
}

pub(crate) struct Class {
    pub(crate) size: usize,
    pub(crate) alignment: usize,
    pub(crate) meta: ClassMeta,
}

pub(crate) enum FunctionMeta {
    Native(fn(StructuredReference) -> StructuredValue),
    Bytecode(Arc<[u8]>),
}

pub(crate) struct Function {
    pub(crate) param_class: Arc<Class>,
    pub(crate) return_class: Arc<Class>,
    pub(crate) meta: FunctionMeta,
}

impl Function {
    fn get_padded_sum_size(&self) -> usize {
        max(self.param_class.size, self.return_class.size).next_multiple_of(size_of::<CallFrameHeader>())
    }
}

#[repr(transparent)]
#[derive(Debug, Copy, Clone)]
pub(crate) struct InstructionPtr(*const u8);

impl InstructionPtr {
    pub(crate) fn native() -> Self { Self(null()) }

    pub(crate) fn is_native(&self) -> bool {
        self.0.is_null()
    }
}

impl Deref for InstructionPtr {
    type Target = *const u8;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// heap: 0x00.. and 0x01..
// stack: 0x10
// static: 0x11
pub(crate) struct Execution {
    pub(crate) instruction_ptr: InstructionPtr,

    pub(crate) registers: [usize; size_of::<u8>()],
    pub(crate) local_frame_ptr: *mut u8,
    pub(crate) local_function_idx: usize,

    pub(crate) stack: Box<[u8]>,
    pub(crate) stack_ptr: *mut u8,

    pub(crate) static_mem: Vec<u8>,
    pub(crate) classes: Vec<Arc<Class>>,
    pub(crate) functions: Vec<Arc<Function>>
}

#[derive(Debug, Copy, Clone)]
struct CallFrameHeader {
    upper_frame_ptr: *mut u8,
    upper_function_idx: usize,
    io_ptr: *mut u8,
    return_ptr: InstructionPtr,
}

pub(crate) struct StructuredValue {
    data: Box<[u8]>,
    class: Arc<Class>
}

pub(crate) struct StructuredReference {
    data: *const u8,
    class: Arc<Class>
}

impl StructuredReference {
    pub(crate) unsafe fn clone(&self) -> StructuredValue {
        StructuredValue {
            data: unsafe { ptr::slice_from_raw_parts(self.data, self.class.size).as_ref().unwrap().to_vec().into_boxed_slice() },
            class: self.class.clone(),
        }
    }
}


macro_rules! read_numeric_type {
    ($type_: ident) => { paste! {
        pub(crate) fn [<read_ $type_>](&mut self) -> Result<$type_> {
            self.check_ptr_in_local_func(*self.instruction_ptr, size_of::<$type_>())?;

            unsafe {
                let val = *(*self.instruction_ptr as *const $type_);
                self.instruction_ptr = InstructionPtr(self.instruction_ptr.add(size_of::<$type_>()));
                Ok(val)
            }
        }
    } };
}

impl Execution {
    fn put_register(&mut self, register: u8, value: usize) {
        self.registers[register as usize] = value;
    }

    fn get_register(&mut self, register: u8) -> usize {
        self.registers[register as usize]
    }

    fn force_call(&mut self, idx: usize, param: StructuredValue) -> Result<StructuredValue> {
        // let func = self.get_function(idx)?;
        //
        // param.data.as_ptr().copy_to(self.stack_ptr);
        //
        // self.stack_ptr = unsafe { self.stack_ptr.add(func.get_padded_sum_size()) };
        //
        //
        // Ok(todo!())
    }

    fn push_structured_value(&mut self, value: StructuredValue) -> Result<()> {

        // self.stack_ptr = unsafe { self.stack_ptr.add(func.get_padded_sum_size()) };
    }

    fn call_function(&mut self, idx: usize) -> Result<()> {
        let func = self.get_function(idx)?;

        let io_ptr = self.get_top_ptr(func.get_padded_sum_size())?;

        const HEADER_SIZE: usize = size_of::<CallFrameHeader>();
        let header = CallFrameHeader {
            upper_frame_ptr: self.local_frame_ptr,
            upper_function_idx: self.local_function_idx,
            return_ptr: self.instruction_ptr,
            io_ptr,
        };

        match &func.meta {
            FunctionMeta::Bytecode(instructions) => {
                self.check_ptr_on_stack(self.stack_ptr, HEADER_SIZE)?;

                unsafe {
                    (self.stack_ptr as *mut CallFrameHeader).write(header);
                    self.stack_ptr = self.stack_ptr.byte_add(HEADER_SIZE);
                }

                self.instruction_ptr = InstructionPtr(instructions.as_ptr());
            }
            FunctionMeta::Native(call) => {
                let input = StructuredReference {
                    data: io_ptr,
                    class: func.param_class.clone(),
                };

                self.instruction_ptr = InstructionPtr::native();
                self.local_function_idx = idx;
                let value = call(input);
                self.instruction_ptr = header.return_ptr;
                self.local_function_idx = header.upper_function_idx;

                unsafe {
                    ptr::slice_from_raw_parts_mut(io_ptr, func.return_class.size).as_mut().unwrap().copy_from_slice(&value.data)
                }
            }
        }

        Ok(())
    }

    fn return_function(&mut self) -> Result<()> {
        let header = self.get_local_header()?;

        let func = self.functions.get(self.local_function_idx).ok_or(anyhow!("Function not found"))?;

        let top_ptr = self.get_top_ptr(func.return_class.size)?;

        unsafe {
            top_ptr.copy_to(header.io_ptr, func.return_class.size);
        }

        self.local_function_idx = header.upper_function_idx;
        self.local_frame_ptr = header.upper_frame_ptr;
        self.instruction_ptr = header.return_ptr;

        Ok(())
    }

    fn get_local_function(&self) -> Result<Arc<Function>> {
        self.get_function(self.local_function_idx)
    }

    fn get_function(&self, idx: usize) -> Result<Arc<Function>> {
        self.functions.get(idx).ok_or(anyhow!("Function not found")).cloned()
    }

    fn check_ptr_on_stack(&self, ptr: *const u8, span: usize) -> Result<()> {
        if ptr < self.stack.as_ptr() {
            return Err(anyhow!("Stack underflow"));
        }

        if ptr.wrapping_byte_add(span) > (self.stack.as_ptr().wrapping_byte_add(self.stack.len())) {
            return Err(anyhow!("Stack overflow"));
        }

        Ok(())
    }

    fn check_ptr_in_local_func(&self, ptr: *const u8, span: usize) -> Result<()> {
        let func_meta = &self.get_local_function()?.meta;

        let local_bytecode = match func_meta {
            FunctionMeta::Native(call) =>
                panic!("Local function is native"),
            FunctionMeta::Bytecode(bytecode) => {
                bytecode
            }
        };

        if ptr < local_bytecode.as_ptr() {
            return Err(anyhow!("Stack underflow"));
        }

        if ptr.wrapping_byte_add(span) > (local_bytecode.as_ptr().wrapping_byte_add(self.stack.len())) {
            return Err(anyhow!("Stack overflow"));
        }

        Ok(())
    }

    fn get_local_ptr(&mut self, up_offset: usize) -> Result<*mut u8> {
        unsafe {
            let new_ptr = self.local_frame_ptr.byte_add(up_offset);

            self.check_ptr_on_stack(new_ptr, 0)?;

            Ok(new_ptr)
        }
    }

    fn get_down_ptr(&self, down_offset: usize) -> Result<*mut u8> {
        unsafe {
            let new_ptr = self.local_frame_ptr.byte_sub(down_offset);

            self.check_ptr_on_stack(new_ptr, 0)?;

            Ok(new_ptr)
        }
    }

    fn get_top_ptr(&self, down_offset: usize) -> Result<*mut u8> {
        unsafe {
            let new_ptr = self.stack_ptr.byte_sub(down_offset);

            self.check_ptr_on_stack(new_ptr, 0)?;

            Ok(new_ptr)
        }
    }

    fn get_local_header(&self) -> Result<CallFrameHeader> {
        const HEADER_SIZE: usize = size_of::<CallFrameHeader>();

        let header_ptr = self.get_down_ptr(HEADER_SIZE)? as *const CallFrameHeader;

        let header: CallFrameHeader = unsafe {
            *(header_ptr)
        };

        Ok(header)
    }

    fn skip(&mut self, amount: usize) -> Result<()> {
        self.check_ptr_in_local_func(*self.instruction_ptr, amount)?;
        unsafe {
            self.instruction_ptr = InstructionPtr(self.instruction_ptr.add(amount));
        }
        Ok(())
    }

    read_numeric_type!(u8);
    read_numeric_type!(u16);
    read_numeric_type!(u32);
    read_numeric_type!(u64);
    read_numeric_type!(usize);



    fn read_instruction(&mut self) -> Result<Instruction> {
        Instruction::from_u8(self.read_u8()?).ok_or(anyhow!("Invalid instruction"))
    }

    fn execute_instruction(&mut self, instruction: Instruction) -> Result<()> {
        match instruction {
            Instruction::Exit => {
                unreachable!()
            }
            Instruction::Load => {
                let register = self.read_u8()?;
                let value = self.read_usize()?;
                self.put_register(register, value);
            }
            Instruction::Load64 => {
                let register = self.read_u8()?;
                let value = self.read_u64()?;
                self.put_register(register, value as usize);
            }
            Instruction::Load32 => {
                let register = self.read_u8()?;
                let value = self.read_u64()?;
                self.put_register(register, value as usize);
            }
            Instruction::Load16 => {
                let register = self.read_u8()?;
                let value = self.read_u16()?;
                self.put_register(register, value as usize);
            }
            Instruction::Load8 => {
                let register = self.read_u8()?;
                let value = self.read_u8()?;
                self.put_register(register, value as usize);
            }
            Instruction::Call => {
                let register = self.read_u8()?;

                let func_idx = self.get_register(register);

                self.call_function(func_idx)?;
            }
            Instruction::Return => {
                self.return_function()?;
            }
        }
        Ok(())
    }

    fn execute(&mut self) -> Result<()> {
        loop {
            let instruction = self.read_instruction()?;

            if let Instruction::Exit = instruction {
                break;
            }

            self.execute_instruction(instruction)?;
        }
        Ok(())
    }
}

