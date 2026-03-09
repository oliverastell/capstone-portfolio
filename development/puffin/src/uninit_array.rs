use std::mem::MaybeUninit;

#[macro_export]
macro_rules! uninit_arr {
    [$type_:ty; $size:expr] => {
        unsafe {
            let uninit_arr: [MaybeUninit<$type_>; $size] = [MaybeUninit::uninit().assume_init(); $size];
            let arr: [$type_; $size] = std::mem::transmute(uninit_arr);
            arr
        }
    };
}