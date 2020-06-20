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

/// Executor holds a list of tasks to be processed
pub struct Executor {
    tasks: VecDeque<Arc<Task>>,
}

impl Default for Executor {
    fn default() -> Self {
        Executor {
            tasks: VecDeque::new(),
        }
    }
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

    pub fn sleeping(&self) -> bool {
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
    fn add_task(&mut self, future: Box<dyn Future<Output = ()> + 'static + Send + Unpin>) {
        // store our task
        let task = Arc::new(Task {
            future: Mutex::new(Box::pin(future)),
            state: Mutex::new(true),
        });
        self.tasks.push_back(task);
    }

    pub fn push_task(&mut self, task: Arc<Task>) {
        self.tasks.push_back(task);
    }

    pub fn pop_tasks(&mut self) -> Option<Arc<Task>> {
        let length = self.tasks.len();
        let mut res = None;
        for _ in 0..length {
            let task = self.tasks.pop_front().unwrap();
            if !task.sleeping() {
                res = Some(task);
                break;
            } else {
                self.tasks.push_back(task);
            }
        }
        res
    }
}

lazy_static! {
    static ref GLOBAL_EXECUTOR: Mutex<Box<Executor>> = {
        let m = Executor::default();
        Mutex::new(Box::new(m))
    };
}

/// Give future to global executor to be polled and executed.
pub fn spawn(future: impl Future<Output = ()> + 'static + Send) {
    GLOBAL_EXECUTOR.lock().add_task(Box::new(Box::pin(future)));
}

pub fn run() -> ! {
    loop {
        let _task = GLOBAL_EXECUTOR.lock().pop_tasks();
        if _task.is_some() {
            let task = _task.unwrap();
            if task.sleeping() {
                GLOBAL_EXECUTOR.lock().push_task(task);
            } else {
                task.mark_sleep();
                let mut is_pending = false;
                {
                    let mut future = task.future.lock();
                    // make a waker for our task
                    let waker = waker_ref(&task);
                    // poll our future and give it a waker
                    let context = &mut Context::from_waker(&*waker);
                    if let Poll::Pending = future.as_mut().poll(context) {
                        is_pending = true;
                    }
                }
                if is_pending {
                    GLOBAL_EXECUTOR.lock().push_task(task);
                }
            }
        } else {
            wait_for_interrupt();
        }
    }
}

#[cfg(target_arch = "x86_64")]
fn wait_for_interrupt() {
    x86_64::instructions::interrupts::enable_interrupts_and_hlt();
    x86_64::instructions::interrupts::disable();
}

#[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
fn wait_for_interrupt() {
    unsafe {
        // enable interrupt and disable
        let sie = riscv::sstatus::read().sie();
        riscv::sstatus::set_sie();
        riscv::asm::wfi();
        if !sie {
            riscv::sstatus::clear_sie();
        }
    }
}
