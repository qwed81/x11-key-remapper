use std::collections::{HashMap, VecDeque};
use std::process::Command;
use std::thread;
use std::time::Duration;

use super::child_process::ChildProcessState;
use super::key_map::{Key, KeyMap};
use super::xbridge::{WindowHandle, XBridge, XBridgeEvent};

struct WindowState {
    x: XBridge,
    parent_child_map: HashMap<WindowHandle, WindowHandle>,
    parent_needed_queue: VecDeque<WindowHandle>,
}

pub fn rebind_until_exit(child: ChildProcessState, key_map: KeyMap) {
    let mut state = WindowState {
        x: XBridge::init().unwrap(),
        parent_child_map: HashMap::new(),
        parent_needed_queue: VecDeque::new(),
    };

    let screen = state.x.default_screen();
    state.x.listen_for_window_creation(screen);

    while child.has_exited() == false {
        let event = state.x.wait_next_event();
        match event {
            XBridgeEvent::Expose { parent } => state.handle_parent_expose(parent, &key_map),
            XBridgeEvent::ResizeRequest { parent } | XBridgeEvent::ConfigureRequest { parent } => {
                state.handle_parent_update(parent);
            }
            XBridgeEvent::ReparentNotify { window } => {
                state.handle_window_reparent(window, child.pid(), screen);
            }
            XBridgeEvent::KeyPress { parent, key } => {
                state.handle_key_press(parent, key, &key_map);
            }
            XBridgeEvent::DestroyNotify { window: _ } => (),
        }
    }
}

impl WindowState {
    fn handle_key_press(&mut self, parent: WindowHandle, pressed_key: Key, key_map: &KeyMap) {
        let new_key = match key_map.mapped_key(pressed_key) {
            Some(new_key) => new_key,
            None => pressed_key,
        };

        println!(
            "from {}:{:x} to {}:{:x}",
            pressed_key.code, pressed_key.state, new_key.code, new_key.state
        );

        let child_window = match self.parent_child_map.get(&parent) {
            Some(&child_window) => child_window,
            None => return,
        };

        self.x.send_key_event(child_window, new_key);
    }

    fn handle_parent_update(&mut self, parent: WindowHandle) {
        match self.parent_child_map.get(&parent) {
            Some(&child) => self.x.resize_to_parent(child, parent),
            None => (),
        }
    }

    fn handle_parent_expose(&mut self, parent: WindowHandle, key_map: &KeyMap) {
        match self.parent_child_map.get(&parent) {
            Some(&child) => {
                self.x.resize_to_parent(child, parent);
            }
            None => {
                let child = self
                    .parent_needed_queue
                    .pop_front()
                    .expect("new window exposed without queued child");

                self.parent_child_map.insert(parent, child);
                self.x.reparent_window(child, parent);
                self.x.grab_keys(parent, key_map.clone());
            }
        }
    }

    fn handle_window_reparent(&mut self, window: WindowHandle, watch_pid: u32, screen: i32) {
        let pid = self.x.get_window_pid(window);
        if pid.is_none() || pid.unwrap() != watch_pid {
            println!("new window: {} pid: {:?}", window, pid);
            return;
        }

        let child_window = window;
        let in_queue = self.parent_needed_queue.iter().any(|&w| w == child_window);
        let already_parented = self.parent_child_map.values().any(|&w| w == child_window);
        if in_queue || already_parented {
            return;
        }

        self.x.create_window(screen);
        self.parent_needed_queue.push_back(child_window);
    }
}
