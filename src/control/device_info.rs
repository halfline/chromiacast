use serde_json::Value;

use crate::error::Error;

const MAX_DEVICE_ID_BYTES: usize = 256;
const MAX_DEVICE_NAME_BYTES: usize = 256;
const MAX_DEVICE_MODEL_BYTES: usize = 256;
/// Device metadata returned by `GET_DEVICE_INFO` on an authenticated control
/// channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedDeviceInfo {
    device_id: String,
    friendly_name: Option<String>,
    device_model: Option<String>,
    capabilities: Option<u64>,
    control_notifications: Option<u64>,
}

impl AuthenticatedDeviceInfo {
    /// Receiver-stable ID asserted over the authenticated control channel.
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    /// User-configured receiver name, when supplied.
    pub fn friendly_name(&self) -> Option<&str> {
        self.friendly_name.as_deref()
    }

    /// Receiver product model, when supplied.
    pub fn device_model(&self) -> Option<&str> {
        self.device_model.as_deref()
    }

    /// Receiver capability bits, when supplied.
    pub fn capabilities(&self) -> Option<u64> {
        self.capabilities
    }

    /// Receiver control-notification bits, when supplied.
    pub fn control_notifications(&self) -> Option<u64> {
        self.control_notifications
    }
}
pub(super) fn parse_device_info(value: &Value) -> Result<AuthenticatedDeviceInfo, Error> {
    require_device_info_message_type(value)?;
    let device_id = required_text(value, "deviceId", MAX_DEVICE_ID_BYTES)?;
    let capabilities = aliased_u64(value, "deviceCapabilities", "capabilities")?;
    Ok(AuthenticatedDeviceInfo {
        device_id,
        friendly_name: optional_text(value, "friendlyName", MAX_DEVICE_NAME_BYTES)?,
        device_model: optional_text(value, "deviceModel", MAX_DEVICE_MODEL_BYTES)?,
        capabilities,
        control_notifications: optional_u64(value, "controlNotifications")?,
    })
}

fn require_device_info_message_type(value: &Value) -> Result<(), Error> {
    match message_type(value) {
        Some("DEVICE_INFO" | "GET_DEVICE_INFO") => Ok(()),
        Some(actual) => Err(protocol_error(format!(
            "expected DEVICE_INFO response, received {actual}"
        ))),
        None => Err(protocol_error("DEVICE_INFO response omitted its type")),
    }
}

fn message_type(value: &Value) -> Option<&str> {
    value
        .get("type")
        .or_else(|| value.get("responseType"))
        .and_then(Value::as_str)
}

fn required_text(value: &Value, field: &'static str, maximum: usize) -> Result<String, Error> {
    optional_text(value, field, maximum)?
        .ok_or_else(|| protocol_error(format!("response omitted {field}")))
}

fn optional_text(
    value: &Value,
    field: &'static str,
    maximum: usize,
) -> Result<Option<String>, Error> {
    optional_text_value(value.get(field), field, maximum)
}

fn optional_text_value(
    value: Option<&Value>,
    field: &'static str,
    maximum: usize,
) -> Result<Option<String>, Error> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let value = value
        .as_str()
        .ok_or_else(|| protocol_error(format!("{field} is not a string")))?
        .trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() > maximum || value.chars().any(char::is_control) {
        return Err(protocol_error(format!(
            "{field} is empty, too long, or contains a control character"
        )));
    }
    Ok(Some(value.to_owned()))
}

fn optional_u64(value: &Value, field: &'static str) -> Result<Option<u64>, Error> {
    let Some(value) = value.get(field) else {
        return Ok(None);
    };
    value
        .as_u64()
        .map(Some)
        .ok_or_else(|| protocol_error(format!("{field} is not an unsigned integer")))
}

fn aliased_u64(
    value: &Value,
    preferred: &'static str,
    compatibility: &'static str,
) -> Result<Option<u64>, Error> {
    let preferred_value = optional_u64(value, preferred)?;
    let compatibility_value = optional_u64(value, compatibility)?;
    match (preferred_value, compatibility_value) {
        (Some(left), Some(right)) if left != right => Err(protocol_error(format!(
            "{preferred} and {compatibility} disagree"
        ))),
        (Some(value), _) | (_, Some(value)) => Ok(Some(value)),
        (None, None) => Ok(None),
    }
}
fn protocol_error(message: impl Into<String>) -> Error {
    Error::ProtocolError(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bounded_device_information_and_both_capability_spellings() {
        let value = serde_json::json!({
            "type": "DEVICE_INFO",
            "requestId": 7,
            "deviceId": " 00112233445566778899AABBCCDDEEFF ",
            "friendlyName": " Living Room ",
            "deviceModel": "Chromecast Ultra",
            "deviceCapabilities": 6149,
            "capabilities": 6149,
            "controlNotifications": 1,
        });
        let info = parse_device_info(&value).unwrap();

        assert_eq!(info.device_id(), "00112233445566778899AABBCCDDEEFF");
        assert_eq!(info.friendly_name(), Some("Living Room"));
        assert_eq!(info.device_model(), Some("Chromecast Ultra"));
        assert_eq!(info.capabilities(), Some(6149));
        assert_eq!(info.control_notifications(), Some(1));
    }

    #[test]
    fn accepts_request_echo_device_info_response() {
        let info = parse_device_info(&serde_json::json!({
            "type": "GET_DEVICE_INFO",
            "requestId": 7,
            "deviceId": "00112233445566778899aabbccddeeff",
        }))
        .unwrap();

        assert_eq!(info.device_id(), "00112233445566778899aabbccddeeff");
    }

    #[test]
    fn rejects_conflicting_or_unbounded_device_information() {
        let conflicting = serde_json::json!({
            "type": "GET_DEVICE_INFO",
            "deviceId": "00112233445566778899aabbccddeeff",
            "deviceCapabilities": 1,
            "capabilities": 4,
        });
        assert!(matches!(
            parse_device_info(&conflicting),
            Err(Error::ProtocolError(_))
        ));

        let oversized = serde_json::json!({
            "type": "GET_DEVICE_INFO",
            "deviceId": "x".repeat(MAX_DEVICE_ID_BYTES + 1),
        });
        assert!(matches!(
            parse_device_info(&oversized),
            Err(Error::ProtocolError(_))
        ));
    }
}
