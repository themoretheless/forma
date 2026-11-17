use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread::Thread;

struct ThreadUnparker(Thread);

impl Wake for ThreadUnparker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

pub fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(ThreadUnparker(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::block_on;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::task::Poll;

    #[test]
    fn block_on_resolves_immediate_and_woken_futures() {
        assert_eq!(block_on(std::future::ready(42)), 42);

        let ready = Arc::new(AtomicBool::new(false));
        let ready_clone = Arc::clone(&ready);
        let value = block_on(std::future::poll_fn(move |cx| {
            if ready_clone.load(Ordering::Acquire) {
                Poll::Ready(99usize)
            } else {
                let waker = cx.waker().clone();
                let flag = Arc::clone(&ready_clone);
                std::thread::spawn(move || {
                    flag.store(true, Ordering::Release);
                    waker.wake();
                });
                Poll::Pending
            }
        }));
        assert_eq!(value, 99);
    }
}
