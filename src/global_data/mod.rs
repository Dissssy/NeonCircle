mod consent;
pub use consent::{get_consent, set_consent};
pub fn init() {
    consent::init();
}
pub fn save() {
    consent::save();
}
