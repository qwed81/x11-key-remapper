use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr, CString};
use std::mem::{self, MaybeUninit};
use std::ptr;
use std::thread;
use std::time::Duration;

use x11_dl::xlib::{
    AnyKey, AnyModifier, AnyPropertyType, Atom, CWEventMask, ConfigureRequest, CurrentTime,
    Display, Expose, ExposureMask, False, GrabModeAsync, KeyPressMask, SubstructureNotifyMask,
    True, Window, XConfigureRequestEvent, XDestroyWindowEvent, XEvent, XExposeEvent, XKeyEvent,
    XReparentEvent, XResizeRequestEvent, XSetWindowAttributes, XWindowAttributes,
};

/*
use x11_dl::xlib::Xlib::{
    XBlackPixel, XChangeWindowAttributes, XCreateSimpleWindow, XDefaultScreen,
    XGetWindowAttributes, XGetWindowProperty, XGrabKey, XInternAtom, XMapWindow, XNextEvent,
    XOpenDisplay, XResizeWindow, XWhitePixel,XFlush,XSync,
    XUngrabKey, XUnmapWindow, XWindowAttributes,XReparentWindow, XRootWindow, XSendEvent,
};
*/
use x11_dl::xlib::Xlib;

use super::key_map::{Key, KeyMap};

// prevent outside from having to import x11 libraries
pub type WindowHandle = Window;

pub enum XBridgeEvent {
    KeyPress { key: Key, parent: WindowHandle },
    Expose { parent: WindowHandle },
    ResizeRequest { parent: WindowHandle },
    ConfigureRequest { parent: WindowHandle },
    ReparentNotify { window: WindowHandle },
    DestroyNotify { window: WindowHandle },
}

pub struct XBridge {
    display: *mut Display,
    grabbed_keys: HashMap<Window, KeyMap>,
    window_creation_listening_screens: Vec<i32>,
    xlib: Xlib,
    cache_atom: Option<Atom>,
}

impl Drop for XBridge {
    fn drop(&mut self) {
        for (window, keys) in &self.grabbed_keys {
            ungrab_keys(&self.xlib, self.display, window.clone(), keys);
        }

        for screen in &self.window_creation_listening_screens {
            free_listen_window_creation(self.display, screen.clone());
        }
    }
}

impl XBridge {
    pub fn init() -> Result<XBridge, ()> {
        let display;
        let xlib = match Xlib::open() {
            Ok(xlib) => xlib,
            Err(_) => return Err(()),
        };

        unsafe {
            display = (xlib.XOpenDisplay)(ptr::null());
            if display.is_null() {
                return Err(());
            }
        }

        Ok(XBridge {
            display,
            xlib,
            grabbed_keys: HashMap::new(),
            window_creation_listening_screens: Vec::new(),
            cache_atom: None,
        })
    }

    pub fn wait_next_event(&self) -> XBridgeEvent {
        unsafe {
            let mut event: MaybeUninit<XEvent> = MaybeUninit::uninit();
            loop {
                (self.xlib.XNextEvent)(self.display, event.as_mut_ptr());

                match event.assume_init().type_ {
                    x11_dl::xlib::KeyPress => {
                        let event = event.as_mut_ptr() as *mut XKeyEvent;
                        let state = (&*event).state;
                        let key_code = (&*event).keycode;
                        return XBridgeEvent::KeyPress {
                            key: Key {
                                state,
                                code: key_code,
                            },
                            parent: (&*event).window,
                        };
                    }
                    x11_dl::xlib::Expose => {
                        let event = event.as_mut_ptr() as *mut XExposeEvent;
                        return XBridgeEvent::Expose {
                            parent: (&*event).window,
                        };
                    }
                    x11_dl::xlib::ResizeRequest => {
                        let event = event.as_mut_ptr() as *mut XResizeRequestEvent;
                        return XBridgeEvent::ResizeRequest {
                            parent: (&*event).window,
                        };
                    }
                    x11_dl::xlib::ConfigureRequest => {
                        let event = event.as_mut_ptr() as *mut XConfigureRequestEvent;
                        return XBridgeEvent::ConfigureRequest {
                            parent: (&*event).window,
                        };
                    }
                    x11_dl::xlib::ReparentNotify => {
                        let event = event.as_mut_ptr() as *mut XReparentEvent;
                        return XBridgeEvent::ReparentNotify {
                            window: (&*event).window,
                        };
                    }
                    x11_dl::xlib::DestroyNotify => {
                        let event = event.as_mut_ptr() as *mut XDestroyWindowEvent;
                        return XBridgeEvent::DestroyNotify {
                            window: (&*event).window,
                        };
                    }
                    _ => {} // we don't need this event, just loop again
                }
            }
        }
    }

    pub fn resize_to_parent(&self, child: WindowHandle, parent: WindowHandle) {
        unsafe {
            let mut attributes: MaybeUninit<XWindowAttributes> = mem::zeroed();
            (self.xlib.XGetWindowAttributes)(self.display, parent, attributes.as_mut_ptr());
            (self.xlib.XSync)(self.display, False);
            let width = attributes.assume_init().width.try_into().unwrap();
            let height = attributes.assume_init().height.try_into().unwrap();

            (self.xlib.XResizeWindow)(self.display, child, width, height);
        }
    }

    pub fn grab_keys(&mut self, window: WindowHandle, key_map: KeyMap) {
        if let Some(keys_map) = self.grabbed_keys.get(&window) {
            // ungrab before removing them, so if there is an error
            // they can still be ungrabbed
            ungrab_keys(&self.xlib, self.display, window, keys_map);
        }

        // if it's there we don't need it anymore, otherwise
        // we can just get rid of it
        self.grabbed_keys.remove(&window);

        /*
        unsafe {
            (self.xlib.XGrabKey)(
                self.display, 46, 0, window, False, GrabModeAsync, GrabModeAsync);
        }
        */

        // grab all of the keys
        for key in key_map.keys() {
            unsafe {
                (self.xlib.XGrabKey)(
                    self.display,
                    key.code.try_into().unwrap(),
                    key.state,
                    window,
                    False,
                    GrabModeAsync,
                    GrabModeAsync,
                );
            }
        }

        // grab the keys before setting the map, so they are not
        // removed if they are never set
        self.grabbed_keys.insert(window, key_map);
    }

    pub fn default_screen(&self) -> i32 {
        unsafe { (self.xlib.XDefaultScreen)(self.display) }
    }

    pub fn create_window(&self, screen: i32) -> WindowHandle {
        unsafe {
            // get the root window
            let root = (self.xlib.XRootWindow)(self.display, screen);
            let black = (self.xlib.XBlackPixel)(self.display, screen);
            let white = (self.xlib.XWhitePixel)(self.display, screen);

            let window =
                (self.xlib.XCreateSimpleWindow)(self.display, root, 0, 0, 1, 1, 1, black, white);

            (self.xlib.XSelectInput)(self.display, window, ExposureMask | KeyPressMask);
            (self.xlib.XMapWindow)(self.display, window);
            window
        }
    }

    pub fn reparent_window(&self, child: WindowHandle, parent: WindowHandle) {
        unsafe {
            (self.xlib.XUnmapWindow)(self.display, child);
            (self.xlib.XMapWindow)(self.display, parent);
            (self.xlib.XSync)(self.display, False);

            (self.xlib.XReparentWindow)(self.display, child, parent, 0, 0);
            (self.xlib.XMapWindow)(self.display, child);

            // allow time for the XServer to receive the
            // events before syncing
            thread::sleep(Duration::from_millis(25));
            (self.xlib.XSync)(self.display, False);
        }
    }

    pub fn send_key_event(&self, window: Window, key: Key) {
        let mut event = XKeyEvent {
            type_: x11_dl::xlib::KeyPress,
            display: self.display,
            window,
            time: CurrentTime,
            same_screen: True,
            send_event: True,
            x: 1,
            y: 1,
            x_root: 1,
            y_root: 1,
            state: key.state,
            keycode: key.code,
            serial: 0,
            root: 0,
            subwindow: 0,
        };

        unsafe {
            // the library expects us to cast to *mut XEvent, with the data of XKeyEvent
            let event_ptr = mem::transmute::<*mut XKeyEvent, *mut XEvent>(&mut event);

            (self.xlib.XSendEvent)(self.display, window, False, KeyPressMask, event_ptr);
            (self.xlib.XFlush)(self.display);
        }
    }

    pub fn get_window_pid(&mut self, window: Window) -> Option<u32> {
        if self.cache_atom.is_none() {
            let atom_name = CString::new("_NET_WM_PID").unwrap();
            let atom = unsafe { (self.xlib.XInternAtom)(self.display, atom_name.as_ptr(), False) };
            self.cache_atom = Some(atom);
        }

        // we know it is there because if it was none, it just got set
        let atom = self.cache_atom.unwrap();

        let mut _actual_type = 0;
        let mut _actual_format = 0;
        let mut _num_items = 0;
        let mut _bytes_after = 0;

        unsafe {
            let mut prop = ptr::null::<u8>() as *mut u8;
            // we do not need any of the other data, as prop is the only
            // one we want. In this case prop will be set to the value of
            // the id, when casted to an u32
            (self.xlib.XGetWindowProperty)(
                self.display,
                window,
                atom,
                0,
                20,
                False,
                6,
                &mut _actual_type,
                &mut _actual_format,
                &mut _num_items,
                &mut _bytes_after,
                &mut prop,
            );

            if prop.is_null() {
                None
            } else {
                let pid = *(prop as *mut u32);
                (self.xlib.XFree)(prop as *mut c_void);
                Some(pid)
            }
        }
    }

    pub fn listen_for_window_creation(&mut self, screen: i32) {
        // guard against listening on already active screens
        for active_screen in &self.window_creation_listening_screens {
            if screen == *active_screen {
                return;
            }
        }

        unsafe {
            // initialize the attributes that will be set on the root window
            let mut attributes: MaybeUninit<XSetWindowAttributes> = mem::zeroed();
            attributes.assume_init_mut().event_mask = SubstructureNotifyMask;

            // get the root window
            let root = (self.xlib.XRootWindow)(self.display, screen);

            // change the root window to now give us notify events on substructure change
            // we can then handle these in our event looop
            (self.xlib.XChangeWindowAttributes)(
                self.display,
                root,
                CWEventMask,
                attributes.as_mut_ptr(),
            );
        }

        self.window_creation_listening_screens.push(screen);
    }
}

fn ungrab_keys(xlib: &Xlib, display: *mut Display, window: Window, key_map: &KeyMap) {
    for key in key_map.keys() {
        unsafe {
            (xlib.XUngrabKey)(display, key.code.try_into().unwrap(), key.state, window);
        }
    }
}

fn free_listen_window_creation(display: *mut Display, screen: i32) {}
