use common::log;
use common::serenity::all::UserId;
use std::{
    collections::{hash_map::Entry, HashMap},
    mem::MaybeUninit,
    sync::atomic::AtomicBool,
};
static mut CONSENT_INFO: MaybeUninit<HashMap<UserId, AtomicBool>> = MaybeUninit::uninit();
static mut INITIALIZED: bool = false;
lazy_static::lazy_static! {
    static ref RWLOCK: std::sync::RwLock<()> = std::sync::RwLock::new(());
}
pub fn init() {
    let file = match std::fs::File::open(crate::config::get_config().consent_path) {
        Ok(f) => f,
        Err(_) => {
            let f = match std::fs::File::create(crate::config::get_config().consent_path) {
                Ok(f) => f,
                Err(e) => panic!("Failed to create consent file: {}", e),
            };
            if let Err(e) = serde_json::to_writer(f, &HashMap::<UserId, bool>::new()) {
                panic!("Failed to write default consent file: {}", e);
            }
            match std::fs::File::open(crate::config::get_config().consent_path) {
                Ok(f) => f,
                Err(e) => panic!("Failed to open consent file: {}", e),
            }
        }
    };
    let res: HashMap<UserId, AtomicBool> = match serde_json::from_reader(file) {
        Ok(r) => r,
        Err(e) => panic!("Failed to read consent file: {}", e),
    };
    unsafe {
        CONSENT_INFO.write(res);
        INITIALIZED = true;
    }
}
pub fn set_consent(user_id: UserId, consent: bool) {
    let init = unsafe { INITIALIZED };
    if !init {
        log::trace!("Database uninitialized when calling set_consent");
        return;
    }
    let map = unsafe { CONSENT_INFO.assume_init_mut() };
    match map.entry(user_id) {
        Entry::Occupied(mut o) => {
            o.get_mut()
                .store(consent, std::sync::atomic::Ordering::Relaxed);
        }
        Entry::Vacant(v) => {
            v.insert(AtomicBool::new(consent));
        }
    }
    save();
}
pub fn get_consent(user_id: UserId) -> bool {
    let init = unsafe { INITIALIZED };
    if !init {
        log::trace!("Database uninitialized when calling get_consent");
        return false;
    }
    let map = unsafe { CONSENT_INFO.assume_init_ref() };
    match map.get(&user_id) {
        Some(b) => b.load(std::sync::atomic::Ordering::Relaxed),
        None => false,
    }
}
pub fn save() {
    let init = unsafe { INITIALIZED };
    if !init {
        log::trace!("Database uninitialized when calling write_map");
        return;
    }
    let file = match std::fs::File::create(crate::config::get_config().consent_path) {
        Ok(f) => f,
        Err(e) => {
            log::error!("Failed to create consent file: {}", e);
            return;
        }
    };
    if let Err(e) = serde_json::to_writer(file, unsafe { CONSENT_INFO.assume_init_ref() }) {
        log::error!("Failed to write consent file: {}", e);
    }
}
