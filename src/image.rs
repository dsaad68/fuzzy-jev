//! States with images in them.
//!
//! A state that holds an image is an array of parts, text and images, in OpenRouter's own shape.
//! `assets`, the shape TypeSafe documents, isn't used, because only Jev reads it everywhere: other
//! models refuse a reference inside text or an object, and Liquid's take a reference that is the
//! whole state as text and answer without having seen the image, a confident answer to the wrong
//! question. Parts are read the same way by every model here that reads images.
//!
//! Ollama's System One endpoint reads none of that: it takes the state as text and the images
//! beside it, in a top-level `images` array of bare base64. [`ollama_shape`] moves them there.

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

/// A state with images in it, in Ollama's shape: the state with its images taken out, and the
/// images' bare base64, in order. Text parts are joined into one string, a blank line between them,
/// and with no text left the state is empty, which Ollama takes. Anything else in the array is kept
/// there, as JSON. A state without images is left as it is.
pub(crate) fn ollama_shape(state: &Value) -> (Value, Vec<String>) {
    let Some(parts) = state.as_array().filter(|_| has_images(state)) else {
        return (state.clone(), Vec::new());
    };
    let mut images = Vec::new();
    let mut rest = Vec::new();
    for part in parts {
        match part.pointer("/image_url/url").and_then(Value::as_str) {
            Some(url) if part["type"] == "image_url" => images.push(url.split_once(";base64,").map_or(url, |(_, data)| data).to_owned()),
            _ => rest.push(part),
        }
    }
    let texts: Option<Vec<&str>> = rest.iter().map(|part| (part["type"] == "text").then(|| part["text"].as_str()).flatten()).collect();
    let state = match texts {
        Some(texts) => Value::String(texts.join("\n\n")),
        None => Value::Array(rest.into_iter().cloned().collect()),
    };
    (state, images)
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
    fn moves_images_out_of_the_state_for_ollama() {
        let png = image_part(b"\x89PNG\r\n\x1a\nrest").unwrap();
        let jpeg = image_part(b"\xff\xd8\xff\xe0").unwrap();
        let (state, images) = ollama_shape(&json!([text_part("A note."), png.clone(), text_part("Another."), jpeg]));
        assert_eq!(state, json!("A note.\n\nAnother."));
        assert_eq!(images, ["iVBORw0KGgpyZXN0", "/9j/4A=="]);

        // Images alone leave an empty state; something other than text stays an array.
        assert_eq!(ollama_shape(&json!([png.clone()])), (json!(""), vec!["iVBORw0KGgpyZXN0".to_owned()]));
        assert_eq!(ollama_shape(&json!([{"order": 7}, png])).0, json!([{"order": 7}]));

        // Without images, nothing changes.
        for state in [json!("text"), json!({"a": 1}), json!([text_part("A note.")])] {
            assert_eq!(ollama_shape(&state), (state.clone(), Vec::new()));
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
