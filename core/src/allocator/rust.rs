use std::{
    alloc::{self, Layout},
    mem, ptr,
};

use super::{Allocator, RawMemPtr};

/// The largest value QuickJS will allocate is a u64;
/// So all allocated memory must have the same alignment is this largest size.
const ALLOC_ALIGN: usize = mem::align_of::<u64>();

#[derive(Copy, Clone)]
#[repr(transparent)]
struct Header {
    size: usize,
}

const fn max(a: usize, b: usize) -> usize {
    if a < b {
        b
    } else {
        a
    }
}

/// Head needs to be at least alloc aligned so all that values after the header are aligned.
const HEADER_SIZE: usize = max(mem::size_of::<Header>(), ALLOC_ALIGN);

#[inline]
fn round_size(size: usize) -> usize {
    (size + ALLOC_ALIGN - 1) / ALLOC_ALIGN * ALLOC_ALIGN
}

/// The allocator which uses Rust global allocator
pub struct RustAllocator;

unsafe impl Allocator for RustAllocator {
    fn alloc(&mut self, size: usize) -> RawMemPtr {
        let size = round_size(size);
        let alloc_size = size + HEADER_SIZE;

        let layout = if let Ok(layout) = Layout::from_size_align(alloc_size, ALLOC_ALIGN) {
            layout
        } else {
            return ptr::null_mut();
        };

        let ptr = unsafe { alloc::alloc(layout) };

        if ptr.is_null() {
            return ptr::null_mut();
        }

        unsafe {
            ptr.cast::<Header>().write(Header { size });
            ptr.add(HEADER_SIZE)
        }
    }

    unsafe fn dealloc(&mut self, ptr: RawMemPtr) {
        let ptr = ptr.sub(HEADER_SIZE);
        let alloc_size = ptr.cast::<Header>().read().size + HEADER_SIZE;
        let layout = Layout::from_size_align_unchecked(alloc_size, ALLOC_ALIGN);

        alloc::dealloc(ptr, layout);
    }

    unsafe fn realloc(&mut self, ptr: RawMemPtr, new_size: usize) -> RawMemPtr {
        let new_size = round_size(new_size);

        let ptr = ptr.sub(HEADER_SIZE);
        let alloc_size = ptr.cast::<Header>().read().size;

        let layout = Layout::from_size_align_unchecked(alloc_size, ALLOC_ALIGN);

        let new_alloc_size = new_size + HEADER_SIZE;

        let ptr = alloc::realloc(ptr, layout, new_alloc_size);

        if ptr.is_null() {
            return ptr::null_mut();
        }

        ptr.cast::<Header>().write(Header { size: new_size });
        ptr.add(HEADER_SIZE)
    }

    unsafe fn usable_size(ptr: RawMemPtr) -> usize {
        let ptr = ptr.sub(HEADER_SIZE);
        ptr.cast::<Header>().read().size
    }
}

#[cfg(all(test, feature = "rust-alloc", feature = "allocator"))]
mod test {
    use super::RustAllocator;
    use crate::{allocator::Allocator, Context, Runtime};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static ALLOC_SIZE: AtomicUsize = AtomicUsize::new(0);

    struct TestAllocator;

    unsafe impl Allocator for TestAllocator {
        fn alloc(&mut self, size: usize) -> crate::allocator::RawMemPtr {
            unsafe {
                let res = RustAllocator.alloc(size);
                ALLOC_SIZE.fetch_add(RustAllocator::usable_size(res), Ordering::AcqRel);
                res
            }
        }

        unsafe fn dealloc(&mut self, ptr: crate::allocator::RawMemPtr) {
            ALLOC_SIZE.fetch_sub(RustAllocator::usable_size(ptr), Ordering::AcqRel);
            RustAllocator.dealloc(ptr);
        }

        unsafe fn realloc(
            &mut self,
            ptr: crate::allocator::RawMemPtr,
            new_size: usize,
        ) -> crate::allocator::RawMemPtr {
            if !ptr.is_null() {
                ALLOC_SIZE.fetch_sub(RustAllocator::usable_size(ptr), Ordering::AcqRel);
            }

            let res = RustAllocator.realloc(ptr, new_size);
            if !res.is_null() {
                ALLOC_SIZE.fetch_add(RustAllocator::usable_size(res), Ordering::AcqRel);
            }
            res
        }

        unsafe fn usable_size(ptr: crate::allocator::RawMemPtr) -> usize
        where
            Self: Sized,
        {
            RustAllocator::usable_size(ptr)
        }
    }

    #[test]
    fn test_gc_working_correctly() {
        let rt = Runtime::new_with_alloc(TestAllocator).unwrap();
        let context = Context::full(&rt).unwrap();

        let before = ALLOC_SIZE.load(Ordering::Acquire);

        context.with(|ctx| {
            ctx.eval::<(), _>(
                r#"
                for(let i = 0;i < 100_000;i++){
                    // create recursive structure.
                    const a = () => {
                        if(a){
                            return true
                        }
                        return false
                    };
                }
            "#,
            )
            .unwrap();
        });

        let after = ALLOC_SIZE.load(Ordering::Acquire);
        // every object takes atleast a single byte.
        // So the gc must have collected atleast some of the recursive objects if the difference is
        // smaller then number of objects created.
        assert!(after.saturating_sub(before) < 100_000)
    }
}
