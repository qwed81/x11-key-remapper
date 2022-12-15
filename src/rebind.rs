use std::collections::{HashMap, VecDeque};
use std::ffi::CString;
use std::process::Command;
use std::thread;
use std::time::Duration;

use super::child_process::ChildProcessState;
use super::key_map::{Key, KeyMap};
use super::xbridge::{WindowHandle, XBridge, XBridgeEvent};

struct DesktopState {
    x: XBridge,
    parent_child_map: HashMap<WindowHandle, WindowState>,
    parent_needed_queue: VecDeque<WindowHandle>,
}

#[derive(PartialEq, Clone, Copy)]
pub enum WindowState {
    Valid(WindowHandle),
    Exiting(WindowHandle),
}

pub struct WindowInfo<'class> {
    pub class: Option<&'class str>,
    pub pid: Option<u32>,
}

pub fn rebind(window_filter: impl Fn(&WindowInfo) -> bool, key_map: KeyMap) {
    let mut state = DesktopState {
        x: XBridge::init().unwrap(),
        parent_child_map: HashMap::new(),
        parent_needed_queue: VecDeque::new(),
    };

    let screen = state.x.default_screen();
    state.x.listen_for_window_creation(screen);

    loop {
        let event = state.x.wait_next_event();
        match event {
            XBridgeEvent::Expose { parent } => state.handle_parent_expose(parent, &key_map),
            XBridgeEvent::ConfigureNotify {
                parent,
                width,
                height,
            } => {
                state.handle_parent_update(parent, width, height);
            }
            XBridgeEvent::ReparentNotify { window } => {
                println!("reparent window: {}", window);

                let pid = state.x.get_window_pid(window);
                let class = state.x.get_window_class(window);
                let class_str = class.as_ref().map(|c| c.to_str().unwrap());
                let info = WindowInfo {
                    pid,
                    class: class_str,
                };

                let pass_filter = window_filter(&info);
                println!("window: {} passed filter: {}", window, pass_filter);

                if pass_filter == false {
                    continue;
                }

                state.handle_window_reparent(window, screen);
            }
            XBridgeEvent::KeyPress { parent, key } => {
                state.handle_key_press(parent, key, &key_map);
            }
            XBridgeEvent::DestroyRequest { window } => {
                println!("destroy request window: {}", window);

                let child_state = match state.parent_child_map.get(&window) {
                    Some(&child) => child,
                    None => continue,
                };

                if let WindowState::Valid(child) = child_state {
                    state.x.notify_child_should_close(child, window);
                    state
                        .parent_child_map
                        .insert(window, WindowState::Exiting(child));
                }
            }
            XBridgeEvent::DestroyNotify { window } => {
                println!("destroy notify window: {}", window);

                // get all keys where it is exiting, and the window
                // is the window that is exiting
                let mut keys = Vec::new();
                for (parent, child_window_state) in state.parent_child_map.iter() {
                    let should_remove = match child_window_state {
                        WindowState::Valid(_) => false,
                        WindowState::Exiting(child_window) => window == *child_window
                    };
                    if should_remove {
                        keys.push(*parent);
                    }
                }

                // clean up all of those values
                for key in keys {
                    state.parent_child_map.remove(&key);
                }
            }
            XBridgeEvent::ParentFocus { parent } => match state.parent_child_map.get(&parent) {
                Some(&child_state) => {
                    if let WindowState::Valid(child) = child_state {
                        state.x.focus_window(child);
                    }
                }
                None => (),
            },
        }
    }
}

impl DesktopState {
    fn handle_key_press(&mut self, parent: WindowHandle, pressed_key: Key, key_map: &KeyMap) {
        let new_key = match key_map.mapped_key(pressed_key) {
            Some(new_key) => new_key,
            None => pressed_key,
        };

        println!(
            "from {}:{:x} to {}:{:x}",
            pressed_key.code, pressed_key.state, new_key.code, new_key.state
        );

        let child_state = match self.parent_child_map.get(&parent) {
            Some(&child_window) => child_window,
            None => return,
        };

        if let WindowState::Valid(child) = child_state {
            self.x.send_key_event(child, new_key);
        }
    }

    fn handle_parent_update(&mut self, parent: WindowHandle, width: u32, height: u32) {
        match self.parent_child_map.get(&parent) {
            Some(&child_state) => {
                if let WindowState::Valid(child) = child_state {
                    self.x.resize_to(child, width, height);
                }
            }
            None => (),
        }
    }

    fn handle_parent_expose(&mut self, parent: WindowHandle, key_map: &KeyMap) {
        println!(
            "parent expose: {}, has child: {}",
            parent,
            self.parent_child_map.get(&parent).is_some()
        );
        match self.parent_child_map.get(&parent) {
            Some(&child_state) => {
                if let WindowState::Valid(child) = child_state {
                    self.x.resize_to_parent(child, parent);
                }
            }
            None => {
                // if the window is exposed and there is no child in the queue
                // that means expose must have come from deletion of the window
                // therefore, it needs to just return
                let child = match self.parent_needed_queue.pop_front() {
                    Some(child) => child,
                    None => return,
                };

                self.parent_child_map
                    .insert(parent, WindowState::Valid(child));
                self.x.reparent_window(child, parent);
                println!("child parented: {}", child);
                self.x.grab_keys(parent, key_map.clone());
            }
        }
    }

    fn handle_window_reparent(&mut self, window: WindowHandle, screen: i32) {
        let child_window = window;
        let in_queue = self.parent_needed_queue.iter().any(|&w| w == child_window);
        let already_parented = self.parent_child_map.values().any(|&state| match state {
            WindowState::Valid(child) => child == window,
            WindowState::Exiting(child) => child == window,
        });

        if in_queue || already_parented {
            println!(
                "window is in queue: {} already parented: {}",
                in_queue, already_parented
            );
            return;
        }

        self.parent_needed_queue.push_back(child_window);
        self.x.create_window(screen);
    }
}
