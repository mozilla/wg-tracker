use lazy_static::lazy_static;

lazy_static! {
    pub static ref CLIENT: reqwest::Client = reqwest::Client::new();
}
