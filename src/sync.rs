use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, Ordering};

pub struct SpinLock<T> {
    next_ticket: AtomicU32,
    now_serving: AtomicU32,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for SpinLock<T> {}
unsafe impl<T: Send> Send for SpinLock<T> {}

pub struct SpinLockGuard<'a, T> {
    lock: &'a SpinLock<T>,
    ticket: u32,
}

impl<T> SpinLock<T> {
    pub const fn new(data: T) -> Self {
        Self {
            next_ticket: AtomicU32::new(0),
            now_serving: AtomicU32::new(0),
            data: UnsafeCell::new(data),
        }
    }

    pub fn lock(&self) -> SpinLockGuard<'_, T> {
        let ticket = self.next_ticket.fetch_add(1, Ordering::Relaxed);
        while self.now_serving.load(Ordering::Acquire) != ticket {
            core::hint::spin_loop(); // WFE on ARM64
        }
        SpinLockGuard { lock: self, ticket }
    }
}

impl<T> core::ops::Deref for SpinLockGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

impl<T> core::ops::DerefMut for SpinLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<T> Drop for SpinLockGuard<'_, T> {
    fn drop(&mut self) {
        self.lock
            .now_serving
            .store(self.ticket + 1, Ordering::Release);
        // SEV wakes any cores spinning in WFE-based spin loops.
        // Currently spin_loop() emits YIELD, but SEV is cheap and
        // future-proofs against switching to WFE.
        #[cfg(target_arch = "aarch64")]
        unsafe { core::arch::asm!("sev", options(nostack, nomem)) };
    }
}
