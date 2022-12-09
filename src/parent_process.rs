use std::collections::HashMap;
use std::time::Duration;

use super::xbridge::{self, XBridgeEvent, WindowHandle};
use super::key_map::KeyMap;

pub struct MasterProcess {
    screen: i32,
    child_process: ChildProcessState,
    parent_child_window_map: HashMap<WindowHandle, WindowHandle>
}

impl MasterProcess {

    pub fn spawn_child(child_command: Command) -> Result<MasterProcess, ()> {
        let display = xbridge::init_display()?;
        xbridge::grab_keys(display, &key_map);

        // this must come before child_process so it can intercept if the process
        // creates a window instantly
        xbridge::init_listen_for_root_window_events(display, screen);

        // if this fails, we have things still running (grab keys) as well
        // as (listening for root_window_events). We need to free them before returning
        // the result
        let child_process = match spawn_child(child_command) {
            Ok(process) => process,
            Err(_) => {
                xbridge::ungrab_keys(display, &key_map);
                xbridge::free_listen_for_root_window_events(display, screen);
                return Err(());
            }
        };

        Ok(MasterProcess {
            display,
            screen,
            child_process,
            key_map,
            parent_child_window_map: HashMap::new()
        })
    }

    pub fn run_event_loop(&mut self) {
        while self.child_process.has_exited() == false {
            let event = xbridge::wait_next_event();
            match event {
                Event::KeyPress { key, parent_window } => {
                    let child = self.parent_child_window_map[&parent_window];
                    if let Some(key) = self.key_map.mapped_key(key) {
                        xbridge::send_key_event(self.display, child, key);
                    }

                },
                Event::Expose => {

                },
                Event::ResizeRequest => {

                },
                Event::ConfigureRequest => {

                },
                Event::ReparentNotify => {

                }

            }
        }
    }

}

