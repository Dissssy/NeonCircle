lazy_static::lazy_static! {
    pub static ref WEB_CLIENT: reqwest::Client = reqwest::Client::new();
}
