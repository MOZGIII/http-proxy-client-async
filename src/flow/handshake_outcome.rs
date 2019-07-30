use crate::http::{HeaderMap, HeaderName, HeaderValue};
use httparse::Response;

#[derive(Debug)]
pub struct ResponseParts {
    pub status_code: u16,
    pub reason_phrase: String,
    pub headers: HeaderMap,
}

/// Panics if response is not complete.
fn parts_from_complete_response<'headers, 'buf: 'headers>(
    response: Response<'headers, 'buf>,
) -> ResponseParts {
    let status_code = response.code.unwrap();
    let reason_phrase = response.reason.unwrap().to_string();
    let mut headers = HeaderMap::new();
    for header in response.headers {
        headers.insert(
            HeaderName::from_bytes(header.name.as_bytes()).unwrap(),
            HeaderValue::from_bytes(header.value).unwrap(),
        );
    }
    ResponseParts {
        status_code,
        reason_phrase,
        headers,
    }
}

#[derive(Debug)]
pub struct HandshakeOutcome {
    pub response_parts: ResponseParts,
    pub data_after_handshake: Vec<u8>,
}

impl HandshakeOutcome {
    pub(crate) fn new<'headers, 'buf: 'headers>(
        response: Response<'headers, 'buf>,
        data_after_handshake: Vec<u8>,
    ) -> Self {
        Self {
            response_parts: parts_from_complete_response(response),
            data_after_handshake,
        }
    }
}
