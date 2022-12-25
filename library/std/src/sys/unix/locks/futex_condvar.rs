use super::Mutex;
use crate::sync::atomic::{AtomicU32, Ordering::Relaxed};
use crate::sys::futex::{futex_wait, futex_wake, futex_wake_all};
use crate::time::Duration;

pub struct Condvar {
    // The value of this atomic is simply incremented on every notification.
    // This is used by `.wait()` to not miss any notifications after
    // unlocking the mutex and before waiting for notifications.
    futex: AtomicU32,
    // The condvar will be permanently broken if the number of waiters exceeds a u32,
    // but that would require having 2^32 threads which I'm going to say is not possible.
    // Furthermore, calls like notify_all only support waking up u32::MAX waiters so
    // this limit is fairly pervasive.
    waiters: AtomicU32,
}

impl Condvar {
    #[inline]
    pub const fn new() -> Self {
        Self { futex: AtomicU32::new(0), waiters: AtomicU32::new(0) }
    }

    // All the memory orderings here are `Relaxed`,
    // because synchronization is done by unlocking and locking the mutex.

    pub fn notify_one(&self) {
        if self.waiters.load(Relaxed) > 0 {
            self.futex.fetch_add(1, Relaxed);
            futex_wake(&self.futex);
        }
    }

    pub fn notify_all(&self) {
        if self.waiters.load(Relaxed) > 0 {
            self.futex.fetch_add(1, Relaxed);
            futex_wake_all(&self.futex);
        }
    }

    pub unsafe fn wait(&self, mutex: &Mutex) {
        self.wait_optional_timeout(mutex, None);
    }

    pub unsafe fn wait_timeout(&self, mutex: &Mutex, timeout: Duration) -> bool {
        self.wait_optional_timeout(mutex, Some(timeout))
    }

    unsafe fn wait_optional_timeout(&self, mutex: &Mutex, timeout: Option<Duration>) -> bool {
        // Examine the notification counter _before_ we unlock the mutex.
        let futex_value = self.futex.load(Relaxed);

        // Register ourselves as waiting _before_ unlocking the mutex since self.futex++
        // only occurs if there's a waiter. The order between this line and the prior is
        // irrelevant since they're both backed by the mutex.
        self.waiters.fetch_add(1, Relaxed);

        // Unlock the mutex before going to sleep.
        mutex.unlock();

        // Wait, but only if there hasn't been any
        // notification since we unlocked the mutex.
        let r = futex_wait(&self.futex, futex_value, timeout);

        // We're no longer waiting: do this as soon as possible to avoid spurious wake calls.
        // Note that calling futex_wake unnecessarily has no effect on correctness,
        // just performance.
        self.waiters.fetch_sub(1, Relaxed);

        // Lock the mutex again.
        mutex.lock();

        r
    }
}

// use std::sync::{Arc, Mutex, Condvar};
// use std::thread;
//
// fn main() {
//     let condvar = Arc::new((Mutex::new(()), Condvar::new()));
//
//     for _ in 0..thread::available_parallelism().unwrap().get() {
//         thread::spawn({
//             let condvar = condvar.clone();
//             move || {
//                 loop {
//                     let guard = condvar.0.lock().unwrap();
//                     drop(condvar.1.wait(guard).unwrap());
//                 }
//             }
//         });
//     }
//
//     fn fibonacci(n: u32) -> u32 {
//         match n {
//             0 => 1,
//             1 => 1,
//             _ => fibonacci(n - 1) + fibonacci(n - 2),
//         }
//     }
//
//     for _ in 0..1_000_000 {
//         condvar.1.notify_one();
//         std::hint::black_box(fibonacci(std::hint::black_box(15)));
//     }
// }
