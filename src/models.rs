use serde::{Deserialize, Serialize};

type Pk = u32;

pub struct Dir {
    pub id: Pk,
    pub parent: Option<Pk>,
}

#[derive(Serialize, Deserialize)]
pub struct Urls {
    pub raw: String,
    pub urls: Vec<String>,
}

#[derive(Serialize)]
pub struct File<'a> {
    pub bundle: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<(u32, u32)>,
}
