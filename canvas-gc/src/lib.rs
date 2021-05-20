use reqwest::{
    blocking::Client,
    header::{HeaderMap, HeaderValue, AUTHORIZATION},
};
use serde::Deserialize;

/**
 * About config:
 * Expects three fields
 * token - Canvas API token can be created in your settings
 * api - API endpoint, typically <https://myschool.instructure.com/api/graphql>
 * cid - Course ID, run `cargo run --bin get_course_id` to list courses
 */

#[derive(Deserialize)]
pub struct Config {
    pub token: String,
    pub api: String,
    pub cid: Option<String>,
}

pub fn read_config() -> Config {
    let r = std::fs::File::open(".config.json").expect("Want a .config.json, see example");
    serde_json::from_reader(r).unwrap()
}

pub fn build_client(token: &str) -> anyhow::Result<Client> {
    let headers = {
        let mut h = HeaderMap::new();
        let mut v = HeaderValue::from_str(&format!("Bearer {}", token))?;
        v.set_sensitive(true);
        h.insert(AUTHORIZATION, v);
        h
    };
    Ok(reqwest::blocking::Client::builder()
        .default_headers(headers)
        .build()?)
}
