use x11_key_remapper::rebind;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (filter, key_map) = x11_key_remapper::parse_args(&args);

    rebind::rebind(filter, key_map);
}
