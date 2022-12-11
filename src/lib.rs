#![allow(unused)]

pub mod child_process;
pub mod key_map;
pub mod rebind;
mod xbridge;


use key_map::KeyMap;
use std::process::Command;

pub fn parse_args() -> (Command, KeyMap) {
    let args: Vec<String> = std::env::args().collect();
    let command = Command::new(&args[1]);
    let key_map = KeyMap::from_file();

    (command, key_map)
}

/*
pub fn run() {
    let (command, key_map) = parse_args();


    let child = child_process::spawn_child(command).unwrap();
    let mut parent_child_map: HashMap<WindowHandle, WindowHandle> = HashMap::new();
    let mut queue = VecDeque::new();

    let mut pid_atom = None;
    let pid = child.pid();
    while child.has_exited() == false {
        let event = x.wait_next_event();
        match event {
            XBridgeEvent::ConfigureRequest { parent_window }
            | XBridgeEvent::ResizeRequest { parent_window }
            | XBridgeEvent::Expose { parent_window } => {
                match parent_child_map.get(&parent_window) {
                    Some(child_window) => {
                        x.resize_to_parent(child_window, parent_window);
                    }
                    None => {
                        let child_window = queue.pop_front();
                        if let None = child_window {
                            println!("no child");
                            continue;
                        }

                        println!("expose first: {}", parent_window);
                        let child_window = child_window.unwrap();
                        parent_child_map.insert(parent_window, child_window);
                        
                        x.reparent_window(child_window, parent_window);

                        println!("grabbing keys");
                        x.grab_keys(parent_window, key_map.clone());
                    }
                }
            }
            XBridgeEvent::ReparentNotify {
                window: child_window,
            } => {
                println!("reparenting {}", child_window);
                let new_pid = x.get_pid_of_window(&mut pid_atom, child_window);
                match new_pid {
                    Some(new_pid) => {
                        if new_pid != pid {
                            println!("new window pid: {:x}, watching pid: {:x}", new_pid, pid);
                            continue;
                        }
                    }
                    None => {
                        println!("pid could not be queried");
                        continue;
                    }
                };

                // it is already in the queue, it does not need to be remapped
                if queue.iter().any(|w| *w == child_window)
                    || parent_child_map.values().any(|w| *w == child_window)
                {
                    println!("already in queue");
                    continue;
                }

                // spawn a new window that will absorb the child that is added
                // to the queue
                let window = x.create_window(screen);
                println!("window created: {}", window);
                queue.push_back(child_window);
            }
            XBridgeEvent::KeyPress { key, parent_window } => {
                println!("key got {} {}", key.code, key.state);
                let child_window = parent_child_map
                    .get(&parent_window)
                    .expect("child not mapped properly")
                    .clone();

                let new_key = match key_map.mapped_key(key) {
                    Some(key) => key,
                    None => key,
                };
                println!("new key {} {}", new_key.code, new_key.state);
                x.send_key_event(child_window, new_key);
            }
        }
    } }
*/
