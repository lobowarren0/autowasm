use std::io;

use crate::capability::CapabilityPolicy;
use crate::service::Service;
use serde_json::{Map, Number, Value};

pub fn compile_service(service: &Service) -> io::Result<Vec<u8>> {
    compile_service_with_policy(service, &CapabilityPolicy::deny_all())
}

pub fn compile_service_with_policy(
    service: &Service,
    policy: &CapabilityPolicy,
) -> io::Result<Vec<u8>> {
    let wat = generate_wat(service, policy)?;

    wat::parse_str(&wat).map_err(|error| io::Error::other(error.to_string()))
}

fn generate_wat(service: &Service, policy: &CapabilityPolicy) -> io::Result<String> {
    let unsupported = service
        .capabilities
        .iter()
        .filter(|capability| !policy.allows(capability))
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    if !unsupported.is_empty() {
        return Err(io::Error::other(format!(
            "unsupported capabilities: {}",
            unsupported.join(", ")
        )));
    }

    if let Some(dynamic) = infer_dynamic_response(service)? {
        return Ok(dynamic_wat(service, &dynamic));
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

struct DynamicResponse {
    route_prefix: String,
    response_prefix: String,
    response_suffix: String,
}

fn infer_dynamic_response(service: &Service) -> io::Result<Option<DynamicResponse>> {
    let marker = "c.req.param(\"";
    let Some(marker_start) = service.handler.find(marker) else {
        return Ok(None);
    };
    let name_start = marker_start + marker.len();
    let name_end = service.handler[name_start..]
        .find("\")")
        .map(|offset| name_start + offset)
        .ok_or_else(|| io::Error::other("malformed route parameter expression"))?;
    let parameter_name = &service.handler[name_start..name_end];
    let route_marker = format!(":{parameter_name}");
    let route_parameter_start = service
        .path
        .find(&route_marker)
        .ok_or_else(|| io::Error::other("route parameter is not present in service path"))?;
    let route_prefix = service.path[..route_parameter_start].to_string();
    if route_prefix.is_empty()
        || service.path[route_parameter_start + route_marker.len()..].contains(':')
    {
        return Err(io::Error::other(
            "only one non-root route parameter is supported",
        ));
    }

    let json_start = service
        .handler
        .find("c.json(")
        .ok_or_else(|| io::Error::other("could not find c.json response"))?
        + "c.json(".len();
    let json_end = service.handler[json_start..]
        .rfind(')')
        .map(|offset| json_start + offset)
        .ok_or_else(|| io::Error::other("malformed c.json response"))?;
    let expression = service.handler[json_start..json_end].trim();
    let dynamic_expression = format!("c.req.param(\"{parameter_name}\")");
    let template = expression.replace(&dynamic_expression, "\"__AUTOWASM_PARAM__\"");
    let body = parse_json_literal(&template)?.to_string();
    let value_marker = "\"__AUTOWASM_PARAM__\"";
    let value_start = body
        .find(value_marker)
        .ok_or_else(|| io::Error::other("route parameter must be a JSON string value"))?;
    let wrapper_prefix = r#"{"status":200,"body":""#;
    let wrapper_suffix = r#""}"#;

    Ok(Some(DynamicResponse {
        route_prefix,
        response_prefix: format!(
            "{}{}",
            wrapper_prefix,
            escape_json_string(&format!("{}\"", &body[..value_start]))
        ),
        response_suffix: format!(
            "{}{}",
            escape_json_string(&format!("\"{}", &body[value_start + value_marker.len()..])),
            wrapper_suffix
        ),
    }))
}

fn dynamic_wat(service: &Service, response: &DynamicResponse) -> String {
    let prefix_length = response.response_prefix.len();
    let suffix_length = response.response_suffix.len();
    let path_offset = format!("{{\"method\":\"{}\",\"path\":\"", service.method).len();
    let suffix_stores = response
                .response_suffix
                .bytes()
                .enumerate()
                .map(|(offset, byte)| {
                        format!(
                                "    local.get $output\n    i32.const {offset}\n    i32.add\n    i32.const {byte}\n    i32.store8\n"
                        )
                })
                .collect::<String>();
    let response_length = format!(
        "(i32.sub (i32.add (local.get $output) (i32.const {suffix_length})) (i32.const 2048))"
    );

    format!(
        r#"(module
    (memory (export "memory") 1)
    (global $heap (mut i32) (i32.const 1024))
    (data (i32.const 2048) "{prefix}")

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
        (local $input i32)
        (local $output i32)
        (local $end i32)
        local.get $ptr
        i32.const {path_offset}
        i32.add
        i32.const {route_prefix_length}
        i32.add
        local.set $input
        local.get $ptr
        local.get $len
        i32.add
        local.set $end
        i32.const 2048
        i32.const {prefix_length}
        i32.add
        local.set $output
        block $done
            loop $copy
                local.get $input
                local.get $end
                i32.ge_u
                br_if $done
                local.get $input
                i32.load8_u
                i32.const 34
                i32.eq
                br_if $done
                local.get $input
                i32.load8_u
                i32.const 47
                i32.eq
                br_if $done
                local.get $output
                local.get $input
                i32.load8_u
                i32.store8
                local.get $input
                i32.const 1
                i32.add
                local.set $input
                local.get $output
                i32.const 1
                i32.add
                local.set $output
                br $copy
            end
        end
{suffix_stores}    i32.const 2048
        i64.extend_i32_u
        i64.const 32
        i64.shl
        {response_length}
        i64.extend_i32_u
        i64.or
    )
)"#,
        prefix = escape_wat_string(&response.response_prefix),
        path_offset = path_offset,
        route_prefix_length = response.route_prefix.len(),
        prefix_length = prefix_length,
        suffix_stores = suffix_stores,
        response_length = response_length,
    )
}

fn infer_static_body(handler: &str) -> io::Result<String> {
    let (marker, expects_string) = if handler.contains("c.json(") {
        ("c.json(", false)
    } else if handler.contains("c.text(") {
        ("c.text(", true)
    } else {
        return Err(io::Error::other(
            "could not find a supported static response",
        ));
    };

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

    let parsed = parse_json_literal(value)?;
    if expects_string && !parsed.is_string() {
        return Err(io::Error::other("c.text requires a static string literal"));
    }

    Ok(if expects_string {
        parsed.as_str().unwrap_or_default().to_string()
    } else {
        parsed.to_string()
    })
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
    use crate::capability::Capability;

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
    fn applies_configured_capability_policy() {
        let service = Service::new(
            "get-hello".to_string(),
            "GET".to_string(),
            "/hello".to_string(),
            r#"(c) => {
  return c.json({ message: "hello" });
}"#
            .to_string(),
            vec![Capability::Network],
        );
        let policy = CapabilityPolicy::allowing([Capability::Network]);

        let wasm = compile_service_with_policy(&service, &policy).unwrap();

        assert_eq!(&wasm[0..4], b"\0asm");
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

    #[test]
    fn compiles_static_text_response() {
        let service = Service::new(
            "get-health".to_string(),
            "GET".to_string(),
            "/health".to_string(),
            r#"(c) => {
  return c.text("ok");
}"#
            .to_string(),
            vec![],
        );

        let wasm = compile_service(&service).unwrap();

        assert_eq!(&wasm[0..4], b"\0asm");
    }
}
