use std::io;

use crate::capability::Capability;
use crate::service::Service;
use serde_json::{Map, Number, Value};

pub fn compile_service(service: &Service) -> io::Result<Vec<u8>> {
    let wat = generate_wat(service)?;

    wat::parse_str(&wat).map_err(|error| io::Error::other(error.to_string()))
}

fn generate_wat(service: &Service) -> io::Result<String> {
    if !service.capabilities.is_empty() {
        return Err(io::Error::other(
            "services with capabilities are not supported by the initial compiler",
        ));
    }

    let body = infer_static_body(&service.handler)?;

    let response = format!(r#"{{"status":200,"body":"{}"}}"#, escape_json_string(&body));

    let response_len = response.len();

    Ok(format!(
        r#"(module
  (memory (export "memory") 1)

  (global $heap (mut i32) (i32.const 1024))

  (data (i32.const 2048) "{escaped_response}")

  (func (export "alloc") (param $size i32) (result i32)
    (local $ptr i32)

    global.get $heap
    local.tee $ptr

    local.get $size
    i32.add

    global.set $heap

    local.get $ptr
  )

  (func (export "handle") (param $ptr i32) (param $len i32) (result i64)
    i64.const {packed}
  )
)"#,
        escaped_response = escape_wat_string(&response),
        packed = pack_pointer_length(2048, response_len),
    ))
}

fn infer_static_body(handler: &str) -> io::Result<String> {
    let marker = "c.json(";

    let start = handler
        .find(marker)
        .ok_or_else(|| io::Error::other("could not find static c.json response"))?;

    let expression = &handler[start + marker.len()..];

    let end = expression
        .rfind(')')
        .ok_or_else(|| io::Error::other("malformed c.json response"))?;

    let value = expression[..end].trim();

    if value.starts_with("await ")
        || value.contains("fetch(")
        || value.contains("request.")
        || value.contains("c.req")
    {
        return Err(io::Error::other(
            "dynamic handlers are not supported by the initial compiler",
        ));
    }

    parse_json_literal(value).map(|value| value.to_string())
}

fn parse_json_literal(source: &str) -> io::Result<Value> {
    let mut parser = JsonLiteralParser::new(source);
    let value = parser.parse_value()?;
    parser.skip_whitespace();

    if parser.position != parser.source.len() {
        return Err(io::Error::other(
            "unsupported expression in c.json response",
        ));
    }

    Ok(value)
}

struct JsonLiteralParser<'a> {
    source: &'a [u8],
    position: usize,
}

impl<'a> JsonLiteralParser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source: source.as_bytes(),
            position: 0,
        }
    }

    fn parse_value(&mut self) -> io::Result<Value> {
        self.skip_whitespace();

        match self.peek() {
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b'"') | Some(b'\'') => self.parse_string().map(Value::String),
            Some(b'-') | Some(b'0'..=b'9') => self.parse_number(),
            Some(_) => self.parse_keyword(),
            None => Err(io::Error::other("missing response expression")),
        }
    }

    fn parse_object(&mut self) -> io::Result<Value> {
        self.expect(b'{')?;
        let mut object = Map::new();
        self.skip_whitespace();

        if self.consume(b'}') {
            return Ok(Value::Object(object));
        }

        loop {
            self.skip_whitespace();
            let key = match self.peek() {
                Some(b'"') | Some(b'\'') => self.parse_string()?,
                Some(_) => self.parse_identifier()?,
                None => return Err(io::Error::other("unterminated response object")),
            };
            self.skip_whitespace();
            self.expect(b':')?;
            object.insert(key, self.parse_value()?);
            self.skip_whitespace();

            if self.consume(b'}') {
                return Ok(Value::Object(object));
            }
            self.expect(b',')?;
        }
    }

    fn parse_array(&mut self) -> io::Result<Value> {
        self.expect(b'[')?;
        let mut values = Vec::new();
        self.skip_whitespace();

        if self.consume(b']') {
            return Ok(Value::Array(values));
        }

        loop {
            values.push(self.parse_value()?);
            self.skip_whitespace();
            if self.consume(b']') {
                return Ok(Value::Array(values));
            }
            self.expect(b',')?;
        }
    }

    fn parse_string(&mut self) -> io::Result<String> {
        let quote = self
            .next()
            .ok_or_else(|| io::Error::other("missing string"))?;
        let mut value = String::new();

        while let Some(character) = self.next() {
            if character == quote {
                return Ok(value);
            }

            if character == b'\\' {
                let escaped = self
                    .next()
                    .ok_or_else(|| io::Error::other("unterminated string escape"))?;
                let decoded = match escaped {
                    b'n' => '\n',
                    b'r' => '\r',
                    b't' => '\t',
                    b'b' => '\u{0008}',
                    b'f' => '\u{000c}',
                    b'\\' => '\\',
                    b'/' => '/',
                    b'"' => '"',
                    b'\'' => '\'',
                    other => other as char,
                };
                value.push(decoded);
            } else {
                value.push(character as char);
            }
        }

        Err(io::Error::other("unterminated response string"))
    }

    fn parse_number(&mut self) -> io::Result<Value> {
        let start = self.position;
        while matches!(
            self.peek(),
            Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
        ) {
            self.position += 1;
        }
        let text = std::str::from_utf8(&self.source[start..self.position])
            .map_err(|_| io::Error::other("invalid response number"))?;
        let number = text
            .parse::<Number>()
            .map_err(|_| io::Error::other("invalid response number"))?;
        Ok(Value::Number(number))
    }

    fn parse_keyword(&mut self) -> io::Result<Value> {
        let identifier = self.parse_identifier()?;
        match identifier.as_str() {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            "null" => Ok(Value::Null),
            _ => Err(io::Error::other(
                "unsupported expression in c.json response",
            )),
        }
    }

    fn parse_identifier(&mut self) -> io::Result<String> {
        let start = self.position;
        while matches!(
            self.peek(),
            Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'$')
        ) {
            self.position += 1;
        }
        if start == self.position {
            return Err(io::Error::other("expected response object key"));
        }
        Ok(String::from_utf8_lossy(&self.source[start..self.position]).into_owned())
    }

    fn skip_whitespace(&mut self) {
        while self
            .peek()
            .is_some_and(|character| character.is_ascii_whitespace())
        {
            self.position += 1;
        }
    }

    fn expect(&mut self, expected: u8) -> io::Result<()> {
        if self.consume(expected) {
            Ok(())
        } else {
            Err(io::Error::other("malformed static c.json response"))
        }
    }

    fn consume(&mut self, expected: u8) -> bool {
        if self.peek() == Some(expected) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<u8> {
        self.source.get(self.position).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let character = self.peek()?;
        self.position += 1;
        Some(character)
    }
}

fn pack_pointer_length(pointer: u32, length: usize) -> u64 {
    ((pointer as u64) << 32) | length as u64
}

fn escape_json_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn escape_wat_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_static_service() {
        let service = Service::new(
            "GET".to_string(),
            "/hello".to_string(),
            "get-hello".to_string(),
            r#"(c) => {
  return c.json({ message: "hello" });
}"#
            .to_string(),
            vec![],
        );

        let wasm = compile_service(&service).unwrap();

        assert_eq!(&wasm[0..4], b"\0asm");
    }

    #[test]
    fn rejects_capability_dependent_service() {
        let service = Service::new(
            "GET".to_string(),
            "/external".to_string(),
            "get-external".to_string(),
            r#"async (c) => {
  const response = await fetch("https://example.com");
  return c.json(await response.json());
}"#
            .to_string(),
            vec![Capability::Network],
        );

        let error = compile_service(&service).unwrap_err();

        assert!(error.to_string().contains("capabilities"));
    }

    #[test]
    fn packs_pointer_and_length() {
        assert_eq!(pack_pointer_length(2048, 26), 8796093022234);
    }

    #[test]
    fn parses_javascript_json_literals() {
        let value = parse_json_literal(
            r#"{ message: 'hello', count: 2, enabled: true, items: [{ id: 1 }] }"#,
        )
        .unwrap();

        assert_eq!(
            value.to_string(),
            r#"{"count":2,"enabled":true,"items":[{"id":1}],"message":"hello"}"#
        );
    }

    #[test]
    fn rejects_dynamic_json_expressions() {
        let error = parse_json_literal("{ message: greeting }").unwrap_err();

        assert!(error.to_string().contains("unsupported expression"));
    }
}
