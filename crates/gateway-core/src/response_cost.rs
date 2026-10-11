//! Bounded JSON cost observation. Payload values are never retained.
use serde_json::Value;

const COST_PATHS: [&[&str]; 6] = [
    &["estimated_cost"],
    &["response_cost"],
    &["usage", "estimated_cost"],
    &["usage", "total_cost"],
    &["usage", "cost"],
    &["_hidden_params", "response_cost"],
];

#[derive(Debug, Clone, Copy)]
enum Expect {
    KeyOrEnd,
    Key,
    Colon,
    ValueOrEnd,
    Value,
    CommaOrEnd,
}

#[derive(Debug)]
struct Frame {
    object: bool,
    expect: Expect,
    path: Vec<String>,
    key: Option<String>,
}

#[derive(Debug)]
enum Token {
    String {
        key: bool,
        raw: Option<Vec<u8>>,
        escape: bool,
        unicode: u8,
        codepoint: u16,
        surrogate: u8,
        utf8: u8,
        next_min: u8,
        next_max: u8,
    },
    Scalar {
        value: Scalar,
        path: Vec<String>,
    },
}

// Validate arbitrarily long scalars without keeping their wire representation.
// Short numbers use serde's exact parser; long numbers keep 64 significant
// decimal digits and an exponent, far beyond f64's significant precision.
#[derive(Debug)]
struct Scalar {
    raw: Vec<u8>,
    state: u8,
    literal: &'static [u8],
    offset: usize,
    digits: String,
    fraction: i64,
    omitted: i64,
    exponent: i64,
    exponent_negative: bool,
    negative: bool,
}

impl Scalar {
    fn new(byte: u8) -> Self {
        let literal: &'static [u8] = match byte {
            b't' => b"true",
            b'f' => b"false",
            b'n' => b"null",
            _ => b"",
        };
        let mut value = Self {
            raw: Vec::new(),
            state: 0,
            literal,
            offset: 0,
            digits: String::new(),
            fraction: 0,
            omitted: 0,
            exponent: 0,
            exponent_negative: false,
            negative: byte == b'-',
        };
        value.feed(byte);
        value
    }

    fn feed(&mut self, byte: u8) {
        if self.raw.len() < 129 {
            self.raw.push(byte);
        }
        if !self.literal.is_empty() {
            if self.literal.get(self.offset) != Some(&byte) {
                self.state = 255;
            }
            self.offset += 1;
            return;
        }
        let previous = self.state;
        self.state = match (previous, byte) {
            (0, b'-') => 1,
            (0 | 1, b'0') => 2,
            (0 | 1, b'1'..=b'9') | (3, b'0'..=b'9') => 3,
            (2 | 3, b'.') => 4,
            (4 | 5, b'0'..=b'9') => 5,
            (2 | 3 | 5, b'e' | b'E') => 6,
            (6, b'+' | b'-') => 7,
            (6..=8, b'0'..=b'9') => 8,
            _ => 255,
        };
        if byte.is_ascii_digit() {
            if matches!(previous, 6..=8) {
                self.exponent = (self.exponent * 10 + i64::from(byte - b'0')).min(1_000_000_000);
            } else {
                if matches!(previous, 4 | 5) {
                    self.fraction += 1;
                }
                if byte != b'0' || !self.digits.is_empty() {
                    if self.digits.len() < 64 {
                        self.digits.push(char::from(byte));
                    } else {
                        self.omitted += 1;
                    }
                }
            }
        } else if previous == 6 && byte == b'-' {
            self.exponent_negative = true;
        }
    }

    fn valid(&self) -> bool {
        if !self.literal.is_empty() {
            self.state != 255 && self.offset == self.literal.len()
        } else {
            matches!(self.state, 2 | 3 | 5 | 8)
        }
    }

    fn cost(&self) -> Option<f64> {
        if !self.literal.is_empty() {
            return None;
        }
        let value = if self.raw.len() <= 128 {
            serde_json::from_slice::<Value>(&self.raw).ok()?.as_f64()?
        } else {
            let exponent = if self.exponent_negative {
                -self.exponent
            } else {
                self.exponent
            };
            format!(
                "{}{}e{}",
                if self.negative { "-" } else { "" },
                if self.digits.is_empty() {
                    "0"
                } else {
                    &self.digits
                },
                exponent - self.fraction + self.omitted
            )
            .parse::<f64>()
            .ok()?
        };
        (value.is_finite() && value > 0.0).then_some(value)
    }
}

#[derive(Debug, Default)]
pub struct ResponseCostObserver {
    stack: Vec<Frame>,
    token: Option<Token>,
    costs: [Option<f64>; 6],
    started: bool,
    done: bool,
    failed: bool,
}

impl ResponseCostObserver {
    pub fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if self.failed {
                break;
            }
            if self.consume_token(byte) {
                continue;
            }
            self.structural(byte);
        }
    }

    pub fn finish(&mut self) {
        if matches!(self.token, Some(Token::Scalar { .. })) {
            self.finish_scalar();
        }
        self.failed |= !self.done || self.token.is_some() || !self.stack.is_empty();
    }

    pub fn cost(&self) -> Option<f64> {
        if self.failed || !self.done {
            return None;
        }
        self.costs.iter().flatten().copied().next()
    }

    pub fn has_input(&self) -> bool {
        self.started || self.failed
    }

    fn consume_token(&mut self, byte: u8) -> bool {
        let Some(token) = self.token.as_mut() else {
            return false;
        };
        match token {
            Token::Scalar { value, .. } => {
                if json_whitespace(byte) || matches!(byte, b',' | b'}' | b']') {
                    self.finish_scalar();
                    return false;
                }
                value.feed(byte);
                self.failed |= value.state == 255;
            }
            Token::String {
                key,
                raw,
                escape,
                unicode,
                codepoint,
                surrogate,
                utf8,
                next_min,
                next_max,
            } => {
                if let Some(buffer) = raw {
                    if buffer.len() < 256 {
                        buffer.push(byte);
                    } else {
                        *raw = None;
                    }
                }
                if *utf8 > 0 {
                    if byte < *next_min || byte > *next_max {
                        self.failed = true;
                    }
                    *utf8 -= 1;
                    *next_min = 0x80;
                    *next_max = 0xbf;
                } else if *unicode > 0 {
                    self.failed |= !byte.is_ascii_hexdigit();
                    *codepoint =
                        (*codepoint << 4) | (char::from(byte).to_digit(16).unwrap_or(0) as u16);
                    *unicode -= 1;
                    if *unicode == 0 {
                        if *surrogate == 3 {
                            self.failed |= !(0xdc00..=0xdfff).contains(codepoint);
                            *surrogate = 0;
                        } else if (0xd800..=0xdbff).contains(codepoint) {
                            *surrogate = 1;
                        } else {
                            self.failed |= (0xdc00..=0xdfff).contains(codepoint);
                        }
                    }
                } else if *surrogate == 1 {
                    self.failed |= byte != b'\\';
                    *surrogate = 2;
                } else if *surrogate == 2 {
                    self.failed |= byte != b'u';
                    *surrogate = 3;
                    *unicode = 4;
                    *codepoint = 0;
                } else if *escape {
                    *escape = false;
                    if byte == b'u' {
                        *unicode = 4;
                        *codepoint = 0;
                    } else {
                        self.failed |=
                            !matches!(byte, b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't');
                    }
                } else if byte == b'"' {
                    let key = *key;
                    let raw = raw.take();
                    self.token = None;
                    if key {
                        match raw.map_or(Some(String::new()), |raw| {
                            serde_json::from_slice::<String>(&raw).ok()
                        }) {
                            Some(key) => {
                                let frame = self.stack.last_mut().expect("key frame");
                                frame.key = Some(key);
                                frame.expect = Expect::Colon;
                            }
                            None => self.failed = true,
                        }
                    }
                } else if byte == b'\\' {
                    *escape = true;
                } else if byte < 0x20 {
                    self.failed = true;
                } else if byte >= 0x80 {
                    (*utf8, *next_min, *next_max) = match byte {
                        0xc2..=0xdf => (1, 0x80, 0xbf),
                        0xe0 => (2, 0xa0, 0xbf),
                        0xe1..=0xec | 0xee..=0xef => (2, 0x80, 0xbf),
                        0xed => (2, 0x80, 0x9f),
                        0xf0 => (3, 0x90, 0xbf),
                        0xf1..=0xf3 => (3, 0x80, 0xbf),
                        0xf4 => (3, 0x80, 0x8f),
                        _ => {
                            self.failed = true;
                            (0, 0x80, 0xbf)
                        }
                    };
                }
            }
        }
        true
    }

    fn value_path(&mut self) -> Option<Vec<String>> {
        let frame = self.stack.last_mut()?;
        if !matches!(frame.expect, Expect::Value | Expect::ValueOrEnd) {
            return None;
        }
        let mut path = frame.path.clone();
        path.push(if frame.object {
            frame.key.take()?
        } else {
            String::new()
        });
        frame.expect = Expect::CommaOrEnd;
        // Duplicate metadata fields replace earlier values, including containers.
        for (index, target) in COST_PATHS.iter().enumerate() {
            if path.len() <= target.len() && path.iter().zip(*target).all(|(a, b)| a == b) {
                self.costs[index] = None;
            }
        }
        // Only two metadata levels are relevant. Do not clone an arbitrary
        // payload's full ancestry into every frame of the bounded JSON stack.
        if !COST_PATHS.iter().any(|target| {
            path.len() <= target.len() && path.iter().zip(*target).all(|(a, b)| a == b)
        }) {
            path = vec![String::new()];
        }
        Some(path)
    }

    fn structural(&mut self, byte: u8) {
        if json_whitespace(byte) {
            return;
        }
        if !self.started {
            self.started = true;
            if byte != b'{' {
                self.failed = true;
                return;
            }
            self.stack.push(Frame {
                object: true,
                expect: Expect::KeyOrEnd,
                path: Vec::new(),
                key: None,
            });
            return;
        }
        if self.done {
            self.failed = true;
            return;
        }
        let Some(frame) = self.stack.last_mut() else {
            self.failed = true;
            return;
        };
        match byte {
            b'"' => {
                let key = frame.object && matches!(frame.expect, Expect::Key | Expect::KeyOrEnd);
                if !key && self.value_path().is_none() {
                    self.failed = true;
                    return;
                }
                self.token = Some(Token::String {
                    key,
                    raw: key.then(|| vec![b'"']),
                    escape: false,
                    unicode: 0,
                    codepoint: 0,
                    surrogate: 0,
                    utf8: 0,
                    next_min: 0x80,
                    next_max: 0xbf,
                });
            }
            b'{' | b'[' => {
                let Some(path) = self.value_path() else {
                    self.failed = true;
                    return;
                };
                if self.stack.len() >= 128 {
                    self.failed = true;
                    return;
                }
                let object = byte == b'{';
                self.stack.push(Frame {
                    object,
                    expect: if object {
                        Expect::KeyOrEnd
                    } else {
                        Expect::ValueOrEnd
                    },
                    path,
                    key: None,
                });
            }
            b'}' | b']' => {
                if frame.object != (byte == b'}')
                    || !matches!(
                        frame.expect,
                        Expect::KeyOrEnd | Expect::ValueOrEnd | Expect::CommaOrEnd
                    )
                {
                    self.failed = true;
                    return;
                }
                self.stack.pop();
                self.done = self.stack.is_empty();
            }
            b':' if frame.object && matches!(frame.expect, Expect::Colon) => {
                frame.expect = Expect::Value
            }
            b',' if matches!(frame.expect, Expect::CommaOrEnd) => {
                frame.expect = if frame.object {
                    Expect::Key
                } else {
                    Expect::Value
                }
            }
            b'-' | b'0'..=b'9' | b't' | b'f' | b'n' => {
                let Some(path) = self.value_path() else {
                    self.failed = true;
                    return;
                };
                self.token = Some(Token::Scalar {
                    value: Scalar::new(byte),
                    path,
                });
            }
            _ => self.failed = true,
        }
    }

    fn finish_scalar(&mut self) {
        let Some(Token::Scalar { value, path }) = self.token.take() else {
            return;
        };
        match value.valid() {
            true => {
                for (index, target) in COST_PATHS.iter().enumerate() {
                    if path.len() == target.len() && path.iter().zip(*target).all(|(a, b)| a == b) {
                        self.costs[index] = value.cost();
                    }
                }
            }
            _ => self.failed = true,
        }
    }
}

fn json_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_responses_account_at_every_chunk_boundary_without_payload_retention() {
        for body in [
            format!(
                "{{\"payload\":\"{}\",\"usage\":{{\"total_cost\":0.42}}}}",
                "x".repeat(100_000)
            ),
            format!(
                "{{\"estimated_cost\":0.42,\"payload\":\"{}\"}}",
                "🙂".repeat(30_000)
            ),
        ] {
            for size in [1, 7, 65_535, 65_536, 100_000] {
                let mut observer = ResponseCostObserver::default();
                for chunk in body.as_bytes().chunks(size) {
                    observer.feed(chunk);
                }
                observer.finish();
                assert_eq!(observer.cost(), Some(0.42), "chunk size {size}");
                assert!(observer.stack.is_empty());
            }
        }
    }

    #[test]
    fn matches_complete_json_cost_semantics_and_rejects_malformed_tail() {
        for body in [
            r#"{"payload":"cost: 100", "other":{"estimated_cost":100},"usage":{"cost":0.2}}"#,
            r#"{"estimated_cost":1,"estimated_cost":null,"usage":{"cost":0.2}}"#,
            r#"{"usage":{"cost":1},"usage":{},"response_cost":0.2}"#,
            r#"{"us\u0061ge":{"total_cost":0.2},"payload":[true,false,null,1e2,{"a":"b"}]}"#,
            r#"{"estimated_cost":0.2} trailing"#,
            r#"{"estimated_cost":0.2,"payload":[1,]}"#,
            r#"{"estimated_cost":0.2,"payload":"unterminated}"#,
            r#"{"estimated_cost":0.2,"payload":"\ud800"}"#,
            r#"{"estimated_cost":0.2,"payload":"\udc00"}"#,
            r#"{"estimated_cost":0.2,"payload":"\ud800\u0041"}"#,
            r#"{"estimated_cost":0.2,"payload":"\ud83d\ude42"}"#,
            "{\"estimated_cost\":0.2,\u{000b}\"payload\":0}",
        ] {
            let mut observer = ResponseCostObserver::default();
            for chunk in body.as_bytes().chunks(3) {
                observer.feed(chunk);
            }
            observer.finish();
            assert_eq!(
                observer.cost(),
                crate::extract_estimated_cost_usd(body.as_bytes()),
                "{body}"
            );
        }
    }

    #[test]
    fn large_keys_and_numeric_payloads_do_not_disable_cost_accounting() {
        for body in [
            format!(
                "{{\"{}\":null,\"usage\":{{\"total_cost\":0.4}}}}",
                "x".repeat(100_000)
            ),
            format!(
                "{{\"payload\":0.{}1,\"response_cost\":0.4}}",
                "0".repeat(100_000)
            ),
            format!("{{\"response_cost\":0.4e{}0}}", "0".repeat(100_000)),
            format!(
                "{{\"payload\":{},\"response_cost\":0.4}}",
                "[".repeat(120) + "0" + &"]".repeat(120)
            ),
        ] {
            let mut observer = ResponseCostObserver::default();
            for chunk in body.as_bytes().chunks(7) {
                observer.feed(chunk);
            }
            observer.finish();
            assert_eq!(observer.cost(), Some(0.4));
        }
    }
}
