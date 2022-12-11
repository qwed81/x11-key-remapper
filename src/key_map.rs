use std::collections::HashMap;

#[derive(Hash, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub code: u32,
    pub state: u32,
}

#[derive(Clone)]
pub struct KeyMap {
    map: HashMap<Key, Key>,
}

impl KeyMap {

    pub fn from_file() -> KeyMap {
        let mut map = HashMap::new();
        let key_from = Key { code: 46, state: 0 };
        let key_to = Key { code: 48, state: 0 };

        map.insert(key_from, key_to);
        KeyMap {
            map
        }
    }

    pub fn keys<'a>(&'a self) -> impl Iterator<Item=&'a Key> {
        self.map.keys()
    }

    pub fn mapped_key(&self, key: Key) -> Option<Key> {
        self.map.get(&key).copied()
    }
}
