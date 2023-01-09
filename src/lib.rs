use std::{
    sync::{mpsc, Arc, Mutex},
    thread,
};

struct Worker {
    id: usize,
    thread: Option<thread::JoinHandle<()>>,
}

impl Worker {
    fn new(id: usize, receiver: Arc<Mutex<mpsc::Receiver<Job>>>) -> Worker {
        // Note: If the operating system can’t create a thread because there aren’t enough system resources,
        // thread::spawn will panic. That will cause our whole server to panic,
        // even though the creation of some threads might succeed.
        // For simplicity’s sake, this behavior is fine,
        // but in a production thread pool implementation, you’d likely want to use std::thread::Builder
        // and its spawn method that returns Result instead.
        let thread = thread::spawn(move || loop {
            // The call to recv blocks, so if there is no job yet,
            // the current thread will wait until a job becomes available.
            // The Mutex<T> ensures that only one Worker thread at a time is trying to request a job.

            let message = receiver.lock().unwrap().recv();

            match message {
                Ok(job) => {
                    println!("Worker {id} got a job; executing.");

                    job();
                }
                Err(_) => {
                    println!("Worker {id} disconnected; shutting down.");
                    break;
                }
            }

            // The code below compiles and runs but doesn’t result in the desired threading behavior:
            // a slow request will still cause other requests to wait to be processed.
            // The reason is somewhat subtle: the Mutex struct has no public unlock method
            // because the ownership of the lock is based on the lifetime of the MutexGuard<T> within the LockResult<MutexGuard<T>>
            // that the lock method returns. At compile time, the borrow checker can then enforce the rule
            // that a resource guarded by a Mutex cannot be accessed unless we hold the lock.
            // However, this implementation can also result in the lock being held longer than intended
            // if we aren’t mindful of the lifetime of the MutexGuard<T>.
            //
            // The code above that uses `let job = receiver.lock().unwrap().recv();`
            // works because with let, any temporary values used in the expression
            // on the right hand side of the equals sign are immediately dropped when the let statement ends.
            // However, `while let` (and if let and match) does not drop temporary values until the end of the associated block.
            // In the code below, the lock remains held for the duration of the call to job(), meaning other workers cannot receive jobs.

            // while let Ok(job) = receiver.lock().unwrap().recv() {
            //     println!("Worker {id} got a job; executing.");

            //     job();
            // }
        });
        Worker {
            id,
            thread: Some(thread),
        }
    }
}

type Job = Box<dyn FnOnce() + Send + 'static>;

pub struct ThreadPool {
    workers: Vec<Worker>,
    sender: Option<mpsc::Sender<Job>>,
}

// Create a new ThreadPool.
//
// The size is the number of threads in the pool.
//
// # Panics
//
// The `new` function will panic if the size is zero.
impl ThreadPool {
    pub fn new(size: usize) -> ThreadPool {
        assert!(size > 0);

        let mut threads = Vec::with_capacity(size);
        let (sender, receiver) = mpsc::channel();
        let receiver = Arc::new(Mutex::new(receiver));

        for id in 0..size {
            threads.push(Worker::new(id, Arc::clone(&receiver)));
        }

        ThreadPool {
            workers: threads,
            sender: Some(sender),
        }
    }
    // we could have a `build` fn instead of `new` returning `Result` instead of panicing
    // but since trying to create a thread pool without any threads is an unrecoverable error
    // we stick to `new`
    // pub fn build(size: usize) -> Result<ThreadPool, PoolCreationError> {}

    pub fn execute<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let job = Box::new(f);
        self.sender.as_ref().unwrap().send(job).unwrap();
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        // Dropping sender closes the channel, which indicates no more messages will be sent.
        drop(self.sender.take());

        for worker in &mut self.workers {
            println!("Shutting down worker {}", worker.id);

            // the `take` method on the Option moves the value out of the Some variant
            // and leaves a None variant in its place.
            // In other words, a Worker that is running will have a Some variant in thread,
            // and when we want to clean up a Worker, we’ll replace Some with None so the Worker doesn’t have a thread to run
            if let Some(thread) = worker.thread.take() {
                thread.join().unwrap();
            }
        }
    }
}
