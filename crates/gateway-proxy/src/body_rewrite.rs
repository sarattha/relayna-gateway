use bytes::Bytes;
use gateway_core::{GatewayError, GatewayResult};
use pingora_http::{RequestHeader, ResponseHeader};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyRewriteOutcome {
    Suppressed,
    Emitted,
}

#[derive(Debug, Clone)]
pub struct BoundedBodyRewriter {
    max_bytes: usize,
    buffer: Vec<u8>,
}

impl BoundedBodyRewriter {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            max_bytes,
            buffer: Vec::new(),
        }
    }

    pub fn buffered_len(&self) -> usize {
        self.buffer.len()
    }

    pub fn filter_chunk<F>(
        &mut self,
        body: &mut Option<Bytes>,
        end_of_stream: bool,
        rewrite: F,
    ) -> GatewayResult<BodyRewriteOutcome>
    where
        F: FnOnce(&[u8]) -> GatewayResult<Vec<u8>>,
    {
        if let Some(chunk) = body.as_ref() {
            let next_len = self.buffer.len().saturating_add(chunk.len());
            if next_len > self.max_bytes {
                return Err(GatewayError::RequestBodyTooLarge);
            }
            self.buffer.extend_from_slice(chunk);
        }

        if end_of_stream {
            let rewritten = rewrite(&self.buffer)?;
            *body = Some(Bytes::from(rewritten));
            self.buffer.clear();
            return Ok(BodyRewriteOutcome::Emitted);
        }

        // Pingora treats `None` as upstream end-of-body. Suppress by emitting
        // an empty chunk instead so callers can continue collecting later chunks.
        *body = Some(Bytes::new());
        Ok(BodyRewriteOutcome::Suppressed)
    }
}

pub fn prepare_rewritten_request_headers(request: &mut RequestHeader, rewritten_len: usize) {
    request.remove_header("Content-Length");
    request.remove_header("Transfer-Encoding");
    let _ = request.insert_header("Content-Length", rewritten_len.to_string());
}

pub fn prepare_http1_rewritten_response_headers(response: &mut ResponseHeader) {
    prepare_rewritten_response_headers(response, true);
}

pub fn prepare_rewritten_response_headers(response: &mut ResponseHeader, use_http1_chunked: bool) {
    response.remove_header("Content-Length");
    response.remove_header("Transfer-Encoding");
    if use_http1_chunked {
        let _ = response.insert_header("Transfer-Encoding", "Chunked");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pingora_http::{RequestHeader, ResponseHeader};

    #[test]
    fn buffers_multi_chunk_request_and_emits_rewritten_final_body() {
        let mut rewriter = BoundedBodyRewriter::new(32);
        let mut first = Some(Bytes::from_static(b"{\"a\""));

        let outcome = rewriter
            .filter_chunk(&mut first, false, |_| unreachable!("not final"))
            .expect("first chunk");

        assert_eq!(outcome, BodyRewriteOutcome::Suppressed);
        assert_eq!(first, Some(Bytes::new()));
        assert_eq!(rewriter.buffered_len(), 4);

        let mut second = Some(Bytes::from_static(b":1}"));
        let outcome = rewriter
            .filter_chunk(&mut second, true, |body| {
                assert_eq!(body, br#"{"a":1}"#);
                Ok(br#"{"a":2}"#.to_vec())
            })
            .expect("final chunk");

        assert_eq!(outcome, BodyRewriteOutcome::Emitted);
        assert_eq!(second, Some(Bytes::from_static(br#"{"a":2}"#)));
        assert_eq!(rewriter.buffered_len(), 0);
    }

    #[test]
    fn suppresses_intermediate_chunks_without_using_none() {
        let mut rewriter = BoundedBodyRewriter::new(32);
        let mut body = Some(Bytes::from_static(b"abc"));

        rewriter
            .filter_chunk(&mut body, false, |_| unreachable!("not final"))
            .expect("chunk");

        assert_eq!(body, Some(Bytes::new()));
    }

    #[test]
    fn response_rewrite_buffers_chunks_and_replaces_final_body() {
        let mut rewriter = BoundedBodyRewriter::new(32);
        let mut first = Some(Bytes::from_static(b"hello "));
        rewriter
            .filter_chunk(&mut first, false, |_| unreachable!("not final"))
            .expect("first chunk");
        assert_eq!(first, Some(Bytes::new()));

        let mut second = Some(Bytes::from_static(b"world"));
        rewriter
            .filter_chunk(&mut second, true, |body| {
                assert_eq!(body, b"hello world");
                Ok(body.iter().map(u8::to_ascii_uppercase).collect())
            })
            .expect("final chunk");

        assert_eq!(second, Some(Bytes::from_static(b"HELLO WORLD")));
    }

    #[test]
    fn body_limit_overflow_returns_gateway_error() {
        let mut rewriter = BoundedBodyRewriter::new(3);
        let mut body = Some(Bytes::from_static(b"abcd"));

        let error = rewriter
            .filter_chunk(&mut body, false, |_| unreachable!("not final"))
            .unwrap_err();

        assert_eq!(error, GatewayError::RequestBodyTooLarge);
    }

    #[test]
    fn empty_and_single_chunk_bodies_emit_final_body() {
        let mut empty_rewriter = BoundedBodyRewriter::new(8);
        let mut empty = None;
        empty_rewriter
            .filter_chunk(&mut empty, true, |body| {
                assert!(body.is_empty());
                Ok(b"{}".to_vec())
            })
            .expect("empty final");
        assert_eq!(empty, Some(Bytes::from_static(b"{}")));

        let mut single_rewriter = BoundedBodyRewriter::new(8);
        let mut single = Some(Bytes::from_static(b"ping"));
        single_rewriter
            .filter_chunk(&mut single, true, |body| {
                assert_eq!(body, b"ping");
                Ok(b"pong".to_vec())
            })
            .expect("single final");
        assert_eq!(single, Some(Bytes::from_static(b"pong")));
    }

    #[test]
    fn request_header_helper_sets_rewritten_content_length() {
        let mut request = RequestHeader::build("POST", b"/v1/responses", None).expect("request");
        request.insert_header("Content-Length", "100").expect("cl");
        request
            .insert_header("Transfer-Encoding", "Chunked")
            .expect("te");

        prepare_rewritten_request_headers(&mut request, 7);

        assert_eq!(
            request
                .headers
                .get("Content-Length")
                .and_then(|v| v.to_str().ok()),
            Some("7")
        );
        assert!(!request.headers.contains_key("Transfer-Encoding"));
    }

    #[test]
    fn response_header_helper_removes_content_length_and_sets_chunked() {
        let mut response = ResponseHeader::build(200, None).expect("response");
        response.insert_header("Content-Length", "100").expect("cl");
        response
            .insert_header("Transfer-Encoding", "Chunked")
            .expect("te");

        prepare_http1_rewritten_response_headers(&mut response);

        assert!(!response.headers.contains_key("Content-Length"));
        assert_eq!(
            response
                .headers
                .get("Transfer-Encoding")
                .and_then(|v| v.to_str().ok()),
            Some("Chunked")
        );
    }

    #[test]
    fn response_header_helper_omits_transfer_encoding_for_non_http1() {
        let mut response = ResponseHeader::build(200, None).expect("response");
        response.insert_header("Content-Length", "100").expect("cl");
        response
            .insert_header("Transfer-Encoding", "Chunked")
            .expect("te");

        prepare_rewritten_response_headers(&mut response, false);

        assert!(!response.headers.contains_key("Content-Length"));
        assert!(!response.headers.contains_key("Transfer-Encoding"));
    }
}
