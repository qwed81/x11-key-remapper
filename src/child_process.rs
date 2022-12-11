use std::process::{Child, Command};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;

pub struct ChildProcessState {
    // an atomic bool can save us from the overhead of using
    // a mutex as all we are trying to do is update the state of a bool
    exited: Arc<AtomicBool>,
    pid: u32,
}

impl ChildProcessState {
    pub fn has_exited(&self) -> bool {
        self.exited.load(Ordering::SeqCst)
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }
}

pub fn spawn_child(mut command: Command) -> Result<ChildProcessState, ()> {
    // spawn the child and receive its id once it
    // returns
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => return Err(()),
    };
    let child_pid = child.id();

    // child exited will be set to true once the child
    // has been exited. This thread waits for the child
    // to exit so it doesn't block the main thread
    let child_exited = Arc::new(AtomicBool::new(false));
    let child_exited_clone = Arc::clone(&child_exited);
    thread::spawn(move || {
        if let Err(_) = child.wait() {
            return Err(());
        }

        // tell that the child has exited
        child_exited_clone.store(true, Ordering::SeqCst);
        Ok(())
    });

    Ok(ChildProcessState {
        exited: child_exited,
        pid: child_pid,
    })
}
