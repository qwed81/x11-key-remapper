use x11_key_remapper::rebind;
use x11_key_remapper::child_process;

fn main() {
    let (command, key_map) = x11_key_remapper::parse_args();
    let child = child_process::spawn_child(command).unwrap();
    rebind::rebind_until_exit(child, key_map);
}
