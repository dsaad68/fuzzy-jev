//! States with images in them.
//!
//! A state that holds an image is an array of parts, text and images, in OpenRouter's own shape.
//! `assets`, the shape TypeSafe documents, isn't used, because only Jev reads it everywhere: other
//! models refuse a reference inside text or an object, and Liquid's take a reference that is the
//! whole state as text and answer without having seen the image, a confident answer to the wrong
//! question. Parts are read the same way by every model here that reads images.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::{json, Value};

use crate::error::{Error, Result};

/// One item of a state's array: text, to sit beside images.
pub fn text_part(text: &str) -> Value {
    json!({"type": "text", "text": text})
}

/// One item of a state's array: an image, from the bytes of a PNG or JPEG file. Anything else is an
/// [`Error::Image`]. How large it may be is left to the endpoint, since that differs by model: Jev
/// refuses a side under 280 pixels, and others take it.
///
/// ```
/// use jev::{image_part, text_part};
///
/// # fn run(photo: &[u8]) -> jev::Result<()> {
/// let state = serde_json::json!([text_part("Returned by a customer."), image_part(photo)?]);
/// # Ok(())
/// # }
/// ```
pub fn image_part(file: &[u8]) -> Result<Value> {
    let mime = mime(file).ok_or_else(|| Error::Image("the endpoint reads PNG and JPEG images, and this is neither".to_owned()))?;
    Ok(json!({"type": "image_url", "image_url": {"url": format!("data:{mime};base64,{}", STANDARD.encode(file))}}))
}

/// Whether `state` has an image in it, which is to say an [`image_part`] among its array's items.
pub fn has_images(state: &Value) -> bool {
    state.as_array().is_some_and(|parts| parts.iter().any(|part| part.get("type").and_then(Value::as_str) == Some("image_url")))
}

/// The image's type, from the first bytes of the file rather than its name.
fn mime(file: &[u8]) -> Option<&'static str> {
    if file.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if file.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_openrouters_parts() {
        assert_eq!(text_part("Is it?"), json!({"type": "text", "text": "Is it?"}));
        assert_eq!(
            image_part(b"\x89PNG\r\n\x1a\nrest").unwrap(),
            json!({"type": "image_url", "image_url": {"url": "data:image/png;base64,iVBORw0KGgpyZXN0"}})
        );
        let jpeg = image_part(b"\xff\xd8\xff\xe0").unwrap();
        assert_eq!(jpeg["image_url"]["url"], json!("data:image/jpeg;base64,/9j/4A=="));
    }

    #[test]
    fn refuses_what_isnt_a_png_or_a_jpeg() {
        for file in [&b"GIF89a"[..], b"RIFF....WEBP", b"", b"\x89PNG"] {
            let refused = image_part(file).unwrap_err();
            assert!(matches!(refused, Error::Image(_)), "{refused}");
        }
    }

    #[test]
    fn finds_images_only_among_a_states_items() {
        let image = image_part(b"\xff\xd8\xff").unwrap();
        assert!(has_images(&json!([text_part("A note."), image])));
        assert!(!has_images(&json!([text_part("A note.")])));
        assert!(!has_images(&json!("image_url")));
        assert!(!has_images(&json!({"type": "image_url"})));
    }
}
