use std::collections::HashMap;
use std::ffi::{c_long, c_char, c_void, CStr, CString};
use std::mem::{self, MaybeUninit};
use std::ptr;
use std::thread;
use std::time::Duration;

use x11_dl::xlib::{
    AnyKey, AnyModifier, AnyPropertyType, Atom, CWEventMask, ConfigureRequest, CurrentTime,
    Display, Expose, ExposureMask, False, GrabModeAsync, KeyPressMask, RevertToPointerRoot,
    StructureNotifyMask, SubstructureNotifyMask, True, Window, XClassHint, XClientMessageEvent,
    XConfigureRequestEvent, XDestroyWindowEvent, XEvent, XExposeEvent, XKeyEvent, XReparentEvent,
    XResizeRequestEvent, XSetWindowAttributes, XWindowAttributes, ClientMessage, ClientMessageData,
    NoEventMask, FocusChangeMask, XEnterWindowEvent, XFocusChangeEvent, NotifyInferior, RevertToNone
};

use x11_dl::xlib::Xlib;

use super::key_map::{Key, KeyMap};

// prevent outside from having to import x11 libraries
pub type WindowHandle = Window;

pub enum XBridgeEvent {
    KeyPress {
        key: Key,
        parent: WindowHandle,
    },
    Expose {
        parent: WindowHandle,
    },
    ConfigureNotify {
        width: u32,
        height: u32,
        parent: WindowHandle,
    },
    ReparentNotify {
        window: WindowHandle,
    },
    DestroyRequest {
        window: WindowHandle,
    },
    DestroyNotify {
        window: WindowHandle,
    },
    ParentFocus {
        parent: WindowHandle
    }
}

pub struct XBridge {
    display: *mut Display,
    grabbed_keys: HashMap<Window, KeyMap>,
    window_creation_listening_screens: Vec<i32>,
    xlib: Xlib,
    pid_atom: Option<Atom>,
    close_window_atom: Atom,
    take_focus_atom: Atom,
    wm_protocols_atom: Atom
}

impl Drop for XBridge {
    fn drop(&mut self) {
        for (&window, keys) in &self.grabbed_keys {
            ungrab_keys(&self.xlib, self.display, window, keys);
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

        let pid_atom = intern_atom(&xlib, display, "_NET_WM_PID");
        let close_window_atom = match intern_atom(&xlib, display, "WM_DELETE_WINDOW") {
            Some(atom) => atom,
            None => return Err(())
        };
        println!("close atom: {}", close_window_atom);

        let take_focus_atom = match intern_atom(&xlib, display, "WM_TAKE_FOCUS") {
            Some(atom) => atom,
            None => return Err(())
        };
        println!("focus atom: {}", take_focus_atom);

        let wm_protocols_atom = match intern_atom(&xlib, display, "WM_PROTOCOLS") {
            Some(atom) => atom,
            None => return Err(())
        };

        Ok(XBridge {
            display,
            xlib,
            grabbed_keys: HashMap::new(),
            window_creation_listening_screens: Vec::new(),
            pid_atom,
            close_window_atom,
            take_focus_atom,
            wm_protocols_atom
        })
    }

    pub fn focus_window(&self, window: WindowHandle) {
        unsafe {
            let mut revert_to = 0;
            let mut focus_window = 0;

            /*
            (self.xlib.XGetInputFocus)(self.display, &mut revert_to, &mut focus_window);
            if window == focus_window.try_into().unwrap() {
                return;
            }
            */

            (self.xlib.XSetInputFocus)(self.display, window, RevertToNone, CurrentTime);
        }
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
                    x11_dl::xlib::ConfigureNotify => {
                        let event = event.as_mut_ptr() as *mut XConfigureRequestEvent;
                        return XBridgeEvent::ConfigureNotify {
                            parent: (&*event).window,
                            width: (&*event).width.try_into().unwrap(),
                            height: (&*event).height.try_into().unwrap(),
                        };
                    }
                    x11_dl::xlib::ReparentNotify => {
                        let event = event.as_mut_ptr() as *mut XReparentEvent;
                        return XBridgeEvent::ReparentNotify {
                            window: (&*event).window,
                        };
                    }
                    x11_dl::xlib::ClientMessage => {
                        let event = event.as_mut_ptr() as *mut XClientMessageEvent;
                        let message_atom = AsMut::<[u64]>::as_mut(&mut (&mut *event).data)[0];

                        if message_atom == self.close_window_atom {
                            return XBridgeEvent::DestroyRequest { window: (&*event).window };
                        }
                        else if message_atom == self.take_focus_atom {
                            todo!();
                        }
                    }
                    x11_dl::xlib::DestroyNotify => {
                        let event = event.as_mut_ptr() as *mut XDestroyWindowEvent;
                        return XBridgeEvent::DestroyNotify {
                            window: (&*event).window
                        }
                    }
                    x11_dl::xlib::FocusIn => {
                        let event = event.as_mut_ptr() as *mut XFocusChangeEvent;

                        // grab keys will cause this event to occur, we want to
                        // filter them out so we can properly know when we need to
                        // refocus the child
                        if (&*event).detail == NotifyInferior {
                            continue;
                        }

                        return XBridgeEvent::ParentFocus {
                            parent: (&*event).window
                        }
                    }
                    _ => {} // we don't need this event, just loop again
                }
            }
        }
    }

    fn kill_message_child() {
        todo!();
    }

    pub fn resize_to_parent(&self, child: WindowHandle, parent: WindowHandle) {
        unsafe {
            let mut attributes: MaybeUninit<XWindowAttributes> = mem::zeroed();
            (self.xlib.XGetWindowAttributes)(self.display, parent, attributes.as_mut_ptr());
            (self.xlib.XSync)(self.display, False);
            let width = attributes.assume_init().width.try_into().unwrap();
            let height = attributes.assume_init().height.try_into().unwrap();

            self.resize_to(child, width, height);
        }
    }

    pub fn resize_to(&self, window: WindowHandle, width: u32, height: u32) {
        unsafe {
            (self.xlib.XResizeWindow)(self.display, window, width, height);
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

    pub fn create_window(&mut self, screen: i32) -> WindowHandle {
        unsafe {
            // get the root window
            let root = (self.xlib.XRootWindow)(self.display, screen);
            let black = (self.xlib.XBlackPixel)(self.display, screen);
            let white = (self.xlib.XWhitePixel)(self.display, screen);

            let window =
                (self.xlib.XCreateSimpleWindow)(self.display, root, 0, 0, 1, 1, 0, black, black);

            (self.xlib.XSelectInput)(
                self.display,
                window,
                StructureNotifyMask | ExposureMask | FocusChangeMask,
            );

            (self.xlib.XMapWindow)(self.display, window);

            // setup receiving the close and resize messages from the wm
            let mut atom_list = [self.take_focus_atom, self.close_window_atom];
            let atom_list_len = atom_list.len() as i32;
            if (self.xlib.XSetWMProtocols)(self.display, window, atom_list.as_mut_ptr(), atom_list_len) == 0 {
                panic!("could not set protocols");
            }

            window
        }
    }

    // sends a request for the child to close, and then calls 
    // destroy window itself
    pub fn notify_child_should_close(&self, child: WindowHandle, parent: WindowHandle) {
        unsafe {
            let root = (self.xlib.XRootWindow)(self.display, self.default_screen());
            (self.xlib.XUnmapWindow)(self.display, child);
            (self.xlib.XSync)(self.display, False);

            (self.xlib.XReparentWindow)(self.display, child, root, 0, 0);

            // allow time for the XServer to receive the
            // events before syncing
            // thread::sleep(Duration::from_millis(1));
            (self.xlib.XSync)(self.display, False);

            (self.xlib.XDestroyWindow)(self.display, parent);
        } 

        let mut client_data = [0; 10];
        client_data[0] = self.close_window_atom as i32;
        client_data[1] = CurrentTime as i32;
        
        unsafe {
            // turn it into the format i can pass to the struct
            let client_data = mem::transmute::<[i32; 10], ClientMessageData>(client_data);

            let mut event = XClientMessageEvent {
                type_: ClientMessage,
                display: self.display,
                send_event: True,
                serial: 0,
                window: child,
                message_type: self.wm_protocols_atom,
                format: 32,
                data: client_data
            };


            let event_ptr = mem::transmute::<*mut XClientMessageEvent, *mut XEvent>(&mut event);
            (self.xlib.XSendEvent)(self.display, child, False, NoEventMask, event_ptr);
        }
    }

    pub fn reparent_window(&self, child: WindowHandle, parent: WindowHandle) {
        unsafe {
            (self.xlib.XUnmapWindow)(self.display, child);
            (self.xlib.XSync)(self.display, False);

            (self.xlib.XReparentWindow)(self.display, child, parent, 0, 0);
            (self.xlib.XMapWindow)(self.display, child);

            // allow time for the XServer to receive the
            // events before syncing
            thread::sleep(Duration::from_millis(1));
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

    pub fn get_window_class(&mut self, window: Window) -> Option<CString> {
        unsafe {
            let mut class_hint: MaybeUninit<XClassHint> = MaybeUninit::uninit();
            let status = (self.xlib.XGetClassHint)(self.display, window, class_hint.as_mut_ptr());

            // if it succeeded, then, return back the string
            if status != 0 {
                (self.xlib.XFree)(class_hint.assume_init().res_name as *mut c_void);
                Some(CString::from_raw(class_hint.assume_init().res_class))
            } else {
                None
            }
        }
    }

    pub fn get_window_pid(&mut self, window: Window) -> Option<u32> {
        let atom = self.pid_atom?;

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

fn intern_atom(xlib: &Xlib, display: *mut Display, atom_name: &'static str) -> Option<Atom> {
    let atom_name = CString::new(atom_name).unwrap();
    let atom = unsafe { (xlib.XInternAtom)(display, atom_name.as_ptr(), False) };
    if atom == 0 { None } else { Some(atom) }
}
