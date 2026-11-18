//! Eklentiye dönen yanıt. Alanlar Windows'taki `carlui_core::vaultlink::Reply`
//! ile birebir aynıdır (eklenti iki sistemde de aynı kodu çalıştırır).

use serde::Serialize;

#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct Login {
    pub id: String,
    pub name: String,
    pub username: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct PasskeyCred {
    pub id: String,
    pub client_data: String,
    pub authenticator_data: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub attestation_object: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub public_key: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub signature: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub user_handle: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct Reply {
    pub ok: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub logins: Vec<Login>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<(String, String)>,
    pub locked: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub status: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub fallback: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passkey: Option<PasskeyCred>,
}

impl Reply {
    pub fn no(message: impl Into<String>) -> Self {
        Self { ok: false, message: message.into(), ..Default::default() }
    }

    /// Passkey isteğini tarayıcıya bırak.
    pub fn fallback(message: impl Into<String>) -> Self {
        Self { fallback: true, ..Self::no(message) }
    }

    pub fn json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".into())
    }
}
