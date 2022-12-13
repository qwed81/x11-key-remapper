use std::collections::{HashMap, VecDeque};
use std::ffi::CString;
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

pub struct WindowInfo<'class> {
    pub class: Option<&'class str>,
    pub pid: Option<u32>,
}

pub fn rebind(window_filter: impl Fn(&WindowInfo) -> bool, key_map: KeyMap) {
    let mut state = WindowState {
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
            XBridgeEvent::ConfigureNotify { parent, width, height } => {
                state.handle_parent_update(parent, width, height);
            }
            XBridgeEvent::ReparentNotify { window } => {
                let pid = state.x.get_window_pid(window);
                let class = state.x.get_window_class(window);
                let class_str = class.as_ref().map(|c| c.to_str().unwrap());
                let info = WindowInfo { pid, class: class_str };

                let pass_filter = window_filter(&info);
                println!("passed filter: {}", pass_filter);
                
                if pass_filter == false {
                    continue;
                }

                state.handle_window_reparent(window, screen);
            }
            XBridgeEvent::KeyPress { parent, key } => {
                state.handle_key_press(parent, key, &key_map);
            }
            XBridgeEvent::DestroyRequest { window: _ } => (),
        }
    }
}

impl WindowState {
    fn handle_key_press(&mut self, parent: WindowHandle, pressed_key: Key, key_map: &KeyMap) {
        let new_key = match key_map.mapped_key(pressed_key) {
            Some(new_key) => new_key,
            None => pressed_key,
        };

        /*
        println!(
            "from {}:{:x} to {}:{:x}",
            pressed_key.code, pressed_key.state, new_key.code, new_key.state
        );
        */

        let child_window = match self.parent_child_map.get(&parent) {
            Some(&child_window) => child_window,
            None => return,
        };

        self.x.send_key_event(child_window, new_key);
    }

    fn handle_parent_update(&mut self, parent: WindowHandle, width: u32, height: u32) {
        match self.parent_child_map.get(&parent) {
            Some(&child) => self.x.resize_to(child, width, height),
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
                self.x.focus_window(child);
            }
        }
    }

    fn handle_window_reparent(&mut self, window: WindowHandle, screen: i32) {
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
