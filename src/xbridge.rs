use std::collections::HashMap;
use std::ffi::{c_char, CString};
use std::mem::{self, MaybeUninit};
use std::ptr;
use std::thread;
use std::time::Duration;

use x11::xlib::{
    AnyPropertyType, Atom, CWEventMask, ConfigureRequest, CurrentTime, Display, Expose, False,
    GrabModeAsync, KeyPress, KeyPressMask, ReparentNotify, ResizeRequest, SubstructureNotifyMask,
    True, Window, XChangeWindowAttributes, XConfigureRequestEvent, XEvent, XExposeEvent, XFlush,
    XGetWindowAttributes, XGetWindowProperty, XGrabKey, XInternAtom, XKeyEvent, XMapWindow,
    XNextEvent, XOpenDisplay, XReparentEvent, XReparentWindow, XResizeRequestEvent, XRootWindow,
    XSendEvent, XSetWindowAttributes, XSync, XUngrabKey, XUnmapWindow, XWindowAttributes,
};

use super::key_map::{Key, KeyMap};

// prevent outside from having to import x11 libraries
pub type WindowHandle = Window;

pub enum XBridgeEvent {
    KeyPress {
        key: Key,
        parent_window: WindowHandle,
    },
    Expose {
        parent_window: WindowHandle,
    },
    ResizeRequest {
        parent_window: WindowHandle,
    },
    ConfigureRequest {
        parent_window: WindowHandle,
    },
    ReparentNotify {
        parent_window: WindowHandle,
    },
}

pub struct XBridge {
    display: *mut Display,
    grabbed_keys: HashMap<Window, KeyMap>,
    window_creation_listening_screens: Vec<i32>,
}

impl Drop for XBridge {
    fn drop(&mut self) {
        for (window, keys) in &self.grabbed_keys {
            ungrab_keys(self.display, window.clone(), keys);
        }

        for screen in self.window_creation_listening_screens {
            free_listen_window_creation(self.display, screen);
        }
    }
}

impl XBridge {
    pub fn init() -> Result<XBridge, ()> {
        let display;
        unsafe {
            display = XOpenDisplay(ptr::null());
            if display as *const Display == ptr::null() {
                return Err(());
            }
        }

        Ok(XBridge {
            display,
            grabbed_keys: HashMap::new(),
            window_creation_listening_screens: Vec::new(),
        })
    }

    pub fn wait_next_event(&self) -> XBridgeEvent {
        unsafe {
            let event: MaybeUninit<XEvent> = MaybeUninit::uninit();
            XNextEvent(self.display, event.as_mut_ptr());

            match event.assume_init().type_ {
                KeyPress => {
                    let event = mem::transmute::<*mut XEvent, *mut XKeyEvent>(event.as_mut_ptr());
                    let state = (&*event).state;
                    let key_code = (&*event).keycode;

                    XBridgeEvent::KeyPress {
                        key: Key {
                            key_state: state,
                            key_code,
                        },
                        parent_window: (&*event).window,
                    }
                }
                Expose => {
                    let event =
                        mem::transmute::<*mut XEvent, *mut XExposeEvent>(event.as_mut_ptr());
                    XBridgeEvent::Expose {
                        parent_window: (&*event).window,
                    }
                }
                ResizeRequest => {
                    let event =
                        mem::transmute::<*mut XEvent, *mut XResizeRequestEvent>(event.as_mut_ptr());
                    XBridgeEvent::ResizeRequest {
                        parent_window: (&*event).window
                    }
                }
                ConfigureRequest => {
                    let event = mem::transmute::<*mut XEvent, *mut XConfigureRequestEvent>(
                        event.as_mut_ptr(),
                    );
                    XBridgeEvent::ResizeRequest {
                        parent_window: (&*event).window
                    }
                }
                ReparentNotify => {
                    let event =
                        mem::transmute::<*mut XEvent, *mut XReparentEvent>(event.as_mut_ptr());
                    XBridgeEvent::ResizeRequest {
                        parent_window: (&*event).window
                    }
                }
            }
        }
    }

    pub fn get_window_bounds(&self, window: WindowHandle) -> (i32, i32) {
        unsafe {
            let attributes: MaybeUninit<XWindowAttributes> = mem::zeroed();
            XGetWindowAttributes(self.display, window, attributes.as_mut_ptr());
            XSync(self.display, False);

            (
                attributes.assume_init().width,
                attributes.assume_init().height,
            )
        }
    }

    pub fn grab_keys(&mut self, window: Window, key_map: KeyMap) {
        if let Some(keys_map) = self.grabbed_keys.get(&window) {
            // ungrab before removing them, so if there is an error
            // they can still be ungrabbed
            ungrab_keys(self.display, window, keys_map);
        }

        // if it's there we don't need it anymore, otherwise
        // we can just get rid of it
        self.grabbed_keys.remove(&window);

        // grab the keys before setting the map, so they are not
        // removed if they are never set
        self.grabbed_keys.insert(window, key_map);

        // grab all of the keys
        for key in key_map.keys() {
            unsafe {
                XGrabKey(
                    self.display,
                    key.key_code.try_into().unwrap(),
                    key.key_state,
                    window,
                    False,
                    GrabModeAsync,
                    GrabModeAsync,
                );
            }
        }
    }

    pub fn reparent_window(&self, child: Window, parent: Window) {
        unsafe {
            XUnmapWindow(self.display, child);
            XMapWindow(self.display, parent);
            XSync(self.display, False);

            XReparentWindow(self.display, child, parent, 0, 0);
            XMapWindow(self.display, child);

            // allow time for the XServer to receive the
            // events before syncing
            thread::sleep(Duration::from_millis(1));
            XSync(self.display, False);
        }
    }

    pub fn send_key_event(&self, window: Window, key: Key) {
        let mut event = XKeyEvent {
            type_: KeyPress,
            display: self.display,
            window,
            time: CurrentTime,
            same_screen: True,
            send_event: True,
            x: 1,
            y: 1,
            x_root: 1,
            y_root: 1,
            state: key.key_state,
            keycode: key.key_code,
            serial: 0,
            root: 0,
            subwindow: 0,
        };

        unsafe {
            // the library expects us to cast to *mut XEvent, with the data of XKeyEvent
            let event_ptr = mem::transmute::<*mut XKeyEvent, *mut XEvent>(&mut event);

            XSendEvent(self.display, window, False, KeyPressMask, event_ptr);
            XFlush(self.display);
        }
    }

    pub fn get_pid_of_window(&self, cache_atom: &mut Option<Atom>, window: Window) -> u32 {
        if cache_atom.is_none() {
            let atom_name = CString::new("_NET_WM_PID").unwrap();
            let atom = unsafe { XInternAtom(self.display, atom_name.as_ptr(), False) };
            *cache_atom = Some(atom);
        }

        // we know it is there because if it was none, it just got set
        let atom = cache_atom.unwrap();
        let mut _actual_type = 0;
        let mut _actual_format = 0;
        let mut _num_items = 0;
        let mut _bytes_after = 0;

        let window_pid = unsafe {
            let mut prop: *mut u8 = ptr::null::<u8>() as *mut u8;
            // we do not need any of the other data, as prop is the only
            // one we want. In this case prop will be set to the value of
            // the id, when casted to an u32
            XGetWindowProperty(
                self.display,
                window,
                atom,
                0,
                0,
                False,
                AnyPropertyType.try_into().unwrap(),
                &mut _actual_type,
                &mut _actual_format,
                &mut _num_items,
                &mut _bytes_after,
                &mut prop,
            );
            *prop as u32
        };

        window_pid
    }

    pub fn listen_for_window_creation(&mut self, screen: i32) {
        // guard against listening on already active screens
        for active_screen in self.window_creation_listening_screens {
            if screen == active_screen {
                return;
            }
        }

        unsafe {
            // initialize the attributes that will be set on the root window
            let mut attributes: MaybeUninit<XSetWindowAttributes> = mem::zeroed();
            attributes.assume_init_mut().event_mask = SubstructureNotifyMask;

            // get the root window
            let root = XRootWindow(self.display, screen);

            // change the root window to now give us notify events on substructure change
            // we can then handle these in our event looop
            XChangeWindowAttributes(self.display, root, CWEventMask, attributes.as_mut_ptr());
        }

        self.window_creation_listening_screens.push(screen);
    }
}

fn ungrab_keys(display: *mut Display, window: Window, key_map: &KeyMap) {
    for key in key_map.keys() {
        unsafe {
            XUngrabKey(display, key.key_code.try_into().unwrap(), key.key_state, window);
        }
    }
}

fn free_listen_window_creation(display: *mut Display, screen: i32) {}
