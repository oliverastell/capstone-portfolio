use crate::uninit_arr;
use paste::paste;
use std::mem::MaybeUninit;
use anyhow::{anyhow, Result};




macro_rules! num_push_pop {
    ($num_type: ident) => { paste! {

        fn [<push_ $num_type>](&mut self, value: $num_type) -> Result<()> {
            let bytes = value.to_ne_bytes();
            self.push(&bytes)
        }

        fn [<pop_ $num_type>](&mut self) -> Result<$num_type> {
            let mut buf = uninit_arr![u8; size_of::<$num_type>()];
            self.pop(&mut buf)?;
            Ok($num_type::from_ne_bytes(buf))
        }

    } };
}
macro_rules! num_put_get {
    ($num_type: ident) => { paste! {

        fn [<put_ $num_type>](&mut self, ptr: usize, value: $num_type) -> Result<()> {
            let bytes = value.to_ne_bytes();
            self.put(ptr, &bytes)
        }

        fn [<get_ $num_type>](&mut self, ptr: usize) -> Result<$num_type> {
            let mut buf = uninit_arr![u8; size_of::<$num_type>()];
            self.get(ptr, &mut buf)?;
            Ok($num_type::from_ne_bytes(buf))
        }

    } };
}

impl VectorPoolStackImpl {
    pub(crate) fn new(size: usize) -> Self {
        Self {
            data: vec![0u8; size],
            stack_ptr: 0,
        }
    }

    pub(crate) fn check_ptr_in_bounds(&self, ptr: usize, size: usize) -> Result<()> {
        if ptr > self.data.len() || ptr + size > self.data.len() {
            return Err(anyhow!("Stack overflow"));
        }

        Ok(())
    }
}

pub(crate) trait StackTrait : PoolTrait {
    fn get_stack_ptr(&self) -> usize;
    fn set_stack_ptr(&mut self, adr: usize);
    unsafe fn push_uninit_unchecked(&mut self, size: usize);
    unsafe fn pop_ignore_unchecked(&mut self, size: usize);
    fn push_uninit(&mut self, size: usize) -> Result<()>;
    fn pop_ignore(&mut self, size: usize) -> Result<()>;
    fn push(&mut self, buf: &[u8]) -> Result<()>;
    fn pop(&mut self, buf: &mut [u8]) -> Result<()>;

    num_push_pop!(usize);
    num_push_pop!(u8);
    num_push_pop!(u16);
    num_push_pop!(u32);
    num_push_pop!(u64);
    num_push_pop!(u128);
}

pub(crate) trait PoolTrait {
    unsafe fn get_unchecked(&self, ptr: usize, buf: &mut [u8]);
    unsafe fn put_unchecked(&mut self, ptr: usize, buf: &[u8]);
    fn get(&self, ptr: usize, buf: &mut [u8]) -> Result<()>;
    fn put(&mut self, ptr: usize, buf: &[u8]) -> Result<()>;

    num_put_get!(usize);
    num_put_get!(u8);
    num_put_get!(u16);
    num_put_get!(u32);
    num_put_get!(u64);
    num_put_get!(u128);
}

pub(crate) type Stack = Box<dyn StackTrait>;
fn new_stack(data: Vec<u8>) -> Stack {
    Box::new(VectorPoolStackImpl {
        data,
        stack_ptr: 0,
    })
}

pub(crate) type Pool = Box<dyn PoolTrait>;
fn new_pool(data: Vec<u8>) -> Pool {
    Box::new(VectorPoolStackImpl {
        data,
        stack_ptr: 0,
    })
}

struct VectorPoolStackImpl {
    data: Vec<u8>,
    stack_ptr: usize, // always in range 0..(data.len())
}

impl PoolTrait for VectorPoolStackImpl {
    unsafe fn get_unchecked(&self, ptr: usize, buf: &mut [u8]) {
        unsafe {
            let end_ptr = ptr.unchecked_add(buf.len());
            buf.copy_from_slice(&self.data.get_unchecked(ptr..end_ptr));
        }
    }

    unsafe fn put_unchecked(&mut self, ptr: usize, buf: &[u8]) {
        unsafe {
            let end_ptr = ptr.unchecked_add(buf.len());

            self.data.get_unchecked_mut(ptr..end_ptr).copy_from_slice(buf);
        }
    }

    fn get(&self, ptr: usize, buf: &mut [u8]) -> Result<()> {
        self.check_ptr_in_bounds(ptr, buf.len())?;
        unsafe {
            self.get_unchecked(ptr, buf);
        }
        Ok(())
    }

    fn put(&mut self, ptr: usize, buf: &[u8]) -> Result<()> {
        self.check_ptr_in_bounds(ptr, buf.len())?;
        unsafe {
            self.put_unchecked(ptr, buf);
        }
        Ok(())
    }
}

impl StackTrait for VectorPoolStackImpl {
    fn get_stack_ptr(&self) -> usize {
        self.stack_ptr
    }

    fn set_stack_ptr(&mut self, adr: usize) {
        self.stack_ptr = adr
    }

    unsafe fn push_uninit_unchecked(&mut self, size: usize) {
        unsafe {
            self.stack_ptr = self.stack_ptr.unchecked_add(size);
        }
    }

    unsafe fn pop_ignore_unchecked(&mut self, size: usize) {
        unsafe {
            self.stack_ptr = self.stack_ptr.unchecked_sub(size);
        }
    }

    fn push_uninit(&mut self, size: usize) -> Result<()> {
        self.check_ptr_in_bounds(self.stack_ptr, size)?;
        unsafe {
            self.push_uninit_unchecked(size);
        }
        Ok(())
    }

    fn pop_ignore(&mut self, size: usize) -> Result<()> {
        self.check_ptr_in_bounds(self.stack_ptr-size, size)?;
        unsafe {
            self.pop_ignore_unchecked(size);
        }
        Ok(())
    }

    fn push(&mut self, buf: &[u8]) -> Result<()> {
        let init = self.stack_ptr;
        self.push_uninit(buf.len())?;
        unsafe {
            self.put_unchecked(init, buf);
        }
        Ok(())
    }

    fn pop(&mut self, buf: &mut [u8]) -> Result<()> {
        self.pop_ignore(buf.len())?;
        unsafe {
            self.get_unchecked(self.stack_ptr, buf);
        }
        Ok(())
    }
}

