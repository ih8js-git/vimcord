use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Guild {
    pub id: String,
    pub name: String,
}
