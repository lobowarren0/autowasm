use std::io;

use crate::capability::CapabilityPolicy;
use crate::service::Service;
use serde_json::{Map, Number, Value};

#[allow(dead_code)]
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

    let response = lower_handler(service)?;

    if let HandlerResponse::Dynamic(dynamic) = response {
        return Ok(dynamic_wat(service, &dynamic));
    }

    let HandlerResponse::Static { body, status } = response else {
        unreachable!("dynamic handler response returned above");
    };

    let response = format!(
        r#"{{"status":{status},"body":"{}"}}"#,
        escape_json_string(&body)
    );

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

enum HandlerResponse {
    Static { body: String, status: u16 },
    Dynamic(DynamicResponse),
}

fn lower_handler(service: &Service) -> io::Result<HandlerResponse> {
    if let Some(dynamic) = infer_dynamic_response(service)? {
        return Ok(HandlerResponse::Dynamic(dynamic));
    }

    let (status, body) = infer_static_response(&service.handler)?;
    Ok(HandlerResponse::Static { body, status })
}

struct DynamicResponse {
    route_prefix: String,
    route_separators: Vec<String>,
    response_prefix: String,
    response_segments: Vec<String>,
}

fn infer_dynamic_response(service: &Service) -> io::Result<Option<DynamicResponse>> {
    let marker = "c.req.param(\"";
    let mut parameter_names = Vec::new();
    let mut search_start = 0;
    while let Some(relative_start) = service.handler[search_start..].find(marker) {
        let marker_start = search_start + relative_start;
        let name_start = marker_start + marker.len();
        let name_end = service.handler[name_start..]
            .find("\")")
            .map(|offset| name_start + offset)
            .ok_or_else(|| io::Error::other("malformed route parameter expression"))?;
        parameter_names.push(service.handler[name_start..name_end].to_string());
        search_start = name_end + 2;
    }

    if parameter_names.is_empty() {
        return Ok(None);
    }

    let mut route_cursor = 0;
    let mut route_prefix = None;
    let mut route_separators = Vec::new();
    for (index, parameter_name) in parameter_names.iter().enumerate() {
        let route_marker = format!(":{parameter_name}");
        let marker_start = service.path[route_cursor..]
            .find(&route_marker)
            .map(|offset| route_cursor + offset)
            .ok_or_else(|| io::Error::other("route parameter is not present in service path"))?;
        if index == 0 {
            route_prefix = Some(service.path[..marker_start].to_string());
        } else {
            route_separators.push(service.path[route_cursor..marker_start].to_string());
        }
        route_cursor = marker_start + route_marker.len();
    }
    route_separators.push(service.path[route_cursor..].to_string());
    let route_prefix = route_prefix.unwrap_or_default();
    if route_prefix.is_empty() {
        return Err(io::Error::other(
            "route parameter must follow a route prefix",
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
    let mut template = expression.to_string();
    for (index, parameter_name) in parameter_names.iter().enumerate() {
        let dynamic_expression = format!("c.req.param(\"{parameter_name}\")");
        template = template.replace(
            &dynamic_expression,
            &format!("\"__AUTOWASM_PARAM_{index}__\""),
        );
    }
    let body = parse_json_literal(&template)?.to_string();
    let mut marker_positions = Vec::new();
    let mut marker_search_start = 0;
    for index in 0..parameter_names.len() {
        let marker = format!("\"__AUTOWASM_PARAM_{index}__\"");
        let value_start = body[marker_search_start..]
            .find(&marker)
            .map(|offset| marker_search_start + offset)
            .ok_or_else(|| io::Error::other("route parameter must be a JSON string value"))?;
        marker_positions.push((value_start, marker.len()));
        marker_search_start = value_start + marker.len();
    }
    let wrapper_prefix = r#"{"status":200,"body":""#;
    let first_marker = marker_positions[0].0;
    let response_prefix = format!(
        "{}{}",
        wrapper_prefix,
        escape_json_string(&format!("{}\"", &body[..first_marker]))
    );
    let wrapper_suffix = r#""}"#;
    let mut response_segments = Vec::new();
    for (index, (value_start, marker_length)) in marker_positions.iter().enumerate() {
        let value_end = value_start + marker_length;
        let segment_end = marker_positions
            .get(index + 1)
            .map(|(next_start, _)| *next_start)
            .unwrap_or(body.len());
        let suffix = if index + 1 == marker_positions.len() {
            wrapper_suffix
        } else {
            "\\\""
        };
        response_segments.push(format!(
            "{}{}",
            escape_json_string(&format!("\"{}", &body[value_end..segment_end])),
            suffix
        ));
    }

    Ok(Some(DynamicResponse {
        route_prefix,
        route_separators,
        response_prefix,
        response_segments,
    }))
}

fn dynamic_wat(service: &Service, response: &DynamicResponse) -> String {
    let prefix_length = response.response_prefix.len();
    let path_offset = format!("{{\"method\":\"{}\",\"path\":\"", service.method).len();
    let mut parameter_blocks = String::new();
    for (index, (route_separator, response_segment)) in response
        .route_separators
        .iter()
        .zip(&response.response_segments)
        .enumerate()
    {
        let static_stores = response_segment
            .bytes()
            .enumerate()
            .map(|(offset, byte)| {
                format!(
                    "        local.get $output\n        i32.const {offset}\n        i32.add\n        i32.const {byte}\n        i32.store8\n"
                )
            })
            .collect::<String>();
        parameter_blocks.push_str(&format!(
            r#"        block $done{index}
            loop $copy{index}
                local.get $input
                local.get $end
                i32.ge_u
                br_if $done{index}
                local.get $input
                i32.load8_u
                i32.const 34
                i32.eq
                br_if $done{index}
                local.get $input
                i32.load8_u
                i32.const 47
                i32.eq
                br_if $done{index}
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
                br $copy{index}
            end
        end
{static_stores}        local.get $output
        i32.const {segment_length}
        i32.add
        local.set $output
        local.get $input
        i32.const {route_separator_length}
        i32.add
        local.set $input
"#,
            static_stores = static_stores,
            segment_length = response_segment.len(),
            route_separator_length = route_separator.len(),
        ));
    }
    let response_length = "(i32.sub (local.get $output) (i32.const 2048))";

    let wat = format!(
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
{parameter_blocks}    i32.const 2048
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
        response_length = response_length,
        parameter_blocks = parameter_blocks,
    );
    wat
}

fn infer_static_response(handler: &str) -> io::Result<(u16, String)> {
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

    let arguments = split_arguments(expression[..end].trim())?;
    let value = arguments
        .first()
        .copied()
        .ok_or_else(|| io::Error::other("missing static response value"))?;

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

    let status = match arguments.get(1) {
        Some(status) => {
            let status = parse_json_literal(status)?;
            let status = status
                .as_u64()
                .ok_or_else(|| io::Error::other("response status must be an integer"))?;
            u16::try_from(status)
                .map_err(|_| io::Error::other("response status is out of range"))?
        }
        None => 200,
    };

    let body = if expects_string {
        parsed.as_str().unwrap_or_default().to_string()
    } else {
        parsed.to_string()
    };

    Ok((status, body))
}

fn split_arguments(source: &str) -> io::Result<Vec<&str>> {
    let mut arguments = Vec::new();
    let mut start = 0;
    let mut depth = 0;
    let mut quote = None;
    let bytes = source.as_bytes();

    for (index, byte) in bytes.iter().copied().enumerate() {
        if let Some(active_quote) = quote {
            if byte == b'\\' {
                continue;
            }
            if byte == active_quote {
                quote = None;
            }
            continue;
        }

        match byte {
            b'"' | b'\'' => quote = Some(byte),
            b'{' | b'[' => depth += 1,
            b'}' | b']' => depth -= 1,
            b',' if depth == 0 => {
                arguments.push(source[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }

    if quote.is_some() || depth != 0 {
        return Err(io::Error::other("malformed static response arguments"));
    }

    arguments.push(source[start..].trim());
    Ok(arguments)
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
    fn lowers_static_and_dynamic_handlers_to_response_ir() {
        let static_service = Service::new(
            "get-hello".to_string(),
            "GET".to_string(),
            "/hello".to_string(),
            r#"(c) => c.json({ message: "hello" })"#.to_string(),
            vec![],
        );
        let dynamic_service = Service::new(
            "get-users-id".to_string(),
            "GET".to_string(),
            "/users/:id".to_string(),
            r#"(c) => c.json({ id: c.req.param("id") })"#.to_string(),
            vec![],
        );

        assert!(matches!(
            lower_handler(&static_service).unwrap(),
            HandlerResponse::Static {
                body: _,
                status: 200
            }
        ));
        assert!(matches!(
            lower_handler(&dynamic_service).unwrap(),
            HandlerResponse::Dynamic(_)
        ));
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

    #[test]
    fn parses_explicit_response_status() {
        let (status, body) =
            infer_static_response(r#"(c) => c.json({ created: true, values: [1, 2] }, 201)"#)
                .unwrap();

        assert_eq!(status, 201);
        assert_eq!(body, r#"{"created":true,"values":[1,2]}"#);

        let (text_status, text_body) =
            infer_static_response(r#"(c) => c.text("ok", 202)"#).unwrap();
        assert_eq!(text_status, 202);
        assert_eq!(text_body, "ok");
    }
}
