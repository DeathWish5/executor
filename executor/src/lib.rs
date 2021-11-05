#![no_std]
extern crate alloc;
pub use executor_macros::*;
use lazy_static::*;
use {
    alloc::{boxed::Box, collections::vec_deque::VecDeque, sync::Arc},
    core::{
        future::Future,
        pin::Pin,
        task::{Context, Poll},
    },
    spin::Mutex,
    woke::{waker_ref, Woke},
};

#[macro_use]
extern crate log;

use log::*;

/// Executor holds a list of tasks to be processed
#[derive(Default)]
pub struct Executor {
    tasks: Mutex<VecDeque<Arc<Task>>>,
}

/// Task is our unit of execution and holds a future are waiting on
pub struct Task {
    pub future: Mutex<Pin<Box<dyn Future<Output = ()> + Send + 'static>>>,
    state: Mutex<bool>,
}

/// Implement what we would like to do when a task gets woken up
impl Woke for Task {
    fn wake_by_ref(task: &Arc<Self>) {
        task.mark_ready();
    }
}

impl Task {
    fn mark_ready(&self) {
        let mut value = self.state.lock();
        *value = true;
    }

    pub fn is_sleeping(&self) -> bool {
        let value = self.state.lock();
        !(*value)
    }

    pub fn mark_sleep(&self) {
        let mut value = self.state.lock();
        *value = false;
    }
}

impl Executor {
    /// Add task for a future to the list of tasks
    fn add_task(&self, future: Pin<Box<dyn Future<Output = ()> + 'static + Send>>) {
        // store our task
        let task = Arc::new(Task {
            future: Mutex::new(future),
            state: Mutex::new(true),
        });
        self.tasks.lock().push_back(task);
    }

    pub fn push_task(&self, task: Arc<Task>) {
        self.tasks.lock().push_back(task);
    }

    pub fn pop_runnable_task(&self) -> Option<Arc<Task>> {
        let mut tasks = self.tasks.lock();
        for _ in 0..tasks.len() {
            let task = tasks.pop_front().unwrap();
            if !task.is_sleeping() {
                return Some(task);
            }
            tasks.push_back(task);
        }
        None
    }

    // Give future to be polled and executed
    pub fn spawn(&self, future: impl Future<Output = ()> + 'static + Send) {
        self.add_task(Box::pin(future));
    }

    /// Spawns a task and blocks the current thread on its result
    pub fn block_on<T, F>(future: impl Future<Output = T> + 'static + Send, mut wait: F) -> T
    where
        F: FnMut(),
    {
        struct NullWaker;
        impl Woke for NullWaker {
            fn wake_by_ref(_: &Arc<Self>) {
                // DO NOTHING
            }
        }
        let null_waker = Arc::new(NullWaker);
        let waker = waker_ref(&null_waker);
        let mut context = Context::from_waker(&*waker);
        let mut future = Box::pin(future);
        loop {
            warn!("Block run a future.");
            if let Poll::Ready(n) = future.as_mut().poll(&mut context) {
                return n;
            }
            wait();
        }
    }

    /// Run futures until there is no runnable task.
    pub fn run_until_idle(&self) {
        while let Some(task) = self.pop_runnable_task() {
            task.mark_sleep();
            // make a waker for our task
            let waker = waker_ref(&task);
            // poll our future and give it a waker
            let mut context = Context::from_waker(&*waker);
            let ret = task.future.lock().as_mut().poll(&mut context);
            if let Poll::Pending = ret {
                self.push_task(task);
            }
        }
    }
}

lazy_static! {
    static ref GLOBAL_EXECUTOR: Executor = Executor::default();
}

/// Give future to global executor to be polled and executed.
pub fn spawn(future: impl Future<Output = ()> + 'static + Send) {
    GLOBAL_EXECUTOR.spawn(future);
}

/// Run futures in global executor until there is no runnable task.
pub fn run_until_idle() {
    GLOBAL_EXECUTOR.run_until_idle();
}
