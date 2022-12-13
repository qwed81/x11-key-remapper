use std::collections::HashMap;
use std::io::BufRead;

#[derive(Hash, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Key {
    pub code: u32,
    pub state: u32,
}

#[derive(Clone)]
pub struct KeyMap {
    map: HashMap<Key, Key>,
}

#[derive(Debug)]
pub enum KeyMapParseError {
    IoError(std::io::Error),
    NotAscii { line_number: usize },
    CouldNotParse { line_number: usize },
    NoKeyPresent { line_number: usize },
    NotValidKey { line_number: usize },
    TooFewArguments { line_number: usize },
    TooManyArguments { line_number: usize },
}

impl From<std::io::Error> for KeyMapParseError {
    fn from(error: std::io::Error) -> KeyMapParseError {
        KeyMapParseError::IoError(error)
    }
}

impl KeyMap {
    pub fn from_stream(mut stream: impl BufRead) -> Result<KeyMap, KeyMapParseError> {
        let mut map = HashMap::new();

        let mut amt_read = 1;
        let mut buffer = String::new();
        let mut line_number = 0;
        while amt_read != 0 {
            buffer.clear();

            amt_read = stream.read_line(&mut buffer)?;
            // println!("buffer: {}", &buffer);
            line_number += 1;
            
            if buffer.is_ascii() == false {
                return Err(KeyMapParseError::NotAscii { line_number });
            }

            let splits: Vec<&str> = buffer.trim().split(' ').collect();

            // if the line starts with a cooment, ignore it
            if splits[0].len() == 0 || &splits[0][0..1] == "#" {
                continue;
            }

            if splits.len() < 2 {
                return Err(KeyMapParseError::TooFewArguments { line_number });
            }
            else if splits.len() > 2 {
                return Err(KeyMapParseError::TooManyArguments { line_number });
            }

            let press_key = parse_split(splits[0], line_number)?;
            let map_key = parse_split(splits[1], line_number)?;
            map.insert(press_key, map_key);

        }

        println!("map is: {:?}", map);
        Ok(KeyMap { map })
    }

    pub fn keys(&self) -> impl Iterator<Item = &Key> {
        self.map.keys()
    }

    pub fn mapped_key(&self, key: Key) -> Option<Key> {
        self.map.get(&key).copied()
    }
}

enum KeyConstant {
    NormalKey { code: u32 },
    ModifierKey { state: u32 },
}

fn parse_split(split: &str, line_number: usize) -> Result<Key, KeyMapParseError>  {
    let keys = split.split('+');
    let mut state = 0;

    for key in keys {
        let parsed_key = parse_key(key);
        match parsed_key {
            Some(parsed_key) => match parsed_key {
                KeyConstant::NormalKey{ code } => {
                    return Ok(Key { code, state })
                },
                KeyConstant::ModifierKey { state: modifier } => {
                    state |= modifier;
                }
            },
            None => {
                return Err(KeyMapParseError::NotValidKey { line_number });
            }
        }
    }

    Err(KeyMapParseError::NoKeyPresent { line_number })
}

fn parse_key(current_string: &str) -> Option<KeyConstant> {
    if let Ok(key_code) = current_string.parse::<u32>() {
        return Some(KeyConstant::NormalKey { code: key_code });
    }

    let modifier = match current_string {
        "Shift" => 0x1,
        "Ctrl" => 0x4,
        "Alt" => 0x8,
        _ => return None,
    };

    Some(KeyConstant::ModifierKey { state: modifier })
}
