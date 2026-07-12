use serde::Serialize;

#[derive(Serialize)]
pub struct Envelope<T: Serialize> {
    pub version: &'static str,
    pub status: &'static str,
    pub data: T,
}

pub fn success<T: Serialize>(data: T) {
    let envelope = Envelope {
        version: "1",
        status: "success",
        data,
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&envelope).unwrap_or_default()
    );
}

pub fn success_with_warnings<T: Serialize, W: Serialize>(data: T, warnings: W) {
    let envelope = serde_json::json!({
        "version": "1",
        "status": "success",
        "data": data,
        "warnings": warnings,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&envelope).unwrap_or_default()
    );
}

pub fn error_with_details(
    code: &str,
    message: &str,
    suggestion: &str,
    details: Option<&serde_json::Value>,
) {
    let mut envelope = serde_json::json!({
        "version": "1",
        "status": "error",
        "error": {
            "code": code,
            "message": message,
            "suggestion": suggestion,
        }
    });
    if let Some(details) = details {
        envelope["error"]["details"] = details.clone();
    }
    eprintln!(
        "{}",
        serde_json::to_string_pretty(&envelope).unwrap_or_default()
    );
}
