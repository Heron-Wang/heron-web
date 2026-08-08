//! JSON 解析器 — 手写最小实现，仅支持项目需要的格式
//! 零第三方依赖，纯标准库实现。

/// 简易 JSON 值枚举
#[derive(Debug, Clone)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<JsonValue>> {
        match self {
            JsonValue::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            JsonValue::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            JsonValue::Object(obj) => {
                for (k, v) in obj {
                    if k == key {
                        return Some(v);
                    }
                }
                None
            }
            _ => None,
        }
    }
}

/// JSON 解析器
pub struct JsonParser<'a> {
    chars: Vec<char>,
    pos: usize,
    _lifetime: std::marker::PhantomData<&'a ()>,
}

impl<'a> JsonParser<'a> {
    pub fn new(input: &'a str) -> Self {
        JsonParser {
            chars: input.chars().collect(),
            pos: 0,
            _lifetime: std::marker::PhantomData,
        }
    }

    pub fn parse(input: &str) -> Result<JsonValue, String> {
        let mut parser = JsonParser {
            chars: input.chars().collect(),
            pos: 0,
            _lifetime: std::marker::PhantomData,
        };
        parser.skip_ws();
        let val = parser.parse_value()?;
        parser.skip_ws();
        Ok(val)
    }

    fn skip_ws(&mut self) {
        while self.pos < self.chars.len() {
            match self.chars[self.pos] {
                ' ' | '\t' | '\n' | '\r' => self.pos += 1,
                _ => break,
            }
        }
    }

    fn parse_value(&mut self) -> Result<JsonValue, String> {
        self.skip_ws();
        if self.pos >= self.chars.len() {
            return Err("unexpected end".to_string());
        }
        match self.chars[self.pos] {
            '"' => self.parse_string(),
            '{' => self.parse_object(),
            '[' => self.parse_array(),
            't' | 'f' => self.parse_bool(),
            'n' => self.parse_null(),
            '-' | '0'..='9' => self.parse_number(),
            _ => Err(format!("unexpected char: {}", self.chars[self.pos])),
        }
    }

    fn parse_string(&mut self) -> Result<JsonValue, String> {
        self.pos += 1; // skip opening "
        let mut out = String::new();
        while self.pos < self.chars.len() {
            match self.chars[self.pos] {
                '"' => {
                    self.pos += 1;
                    return Ok(JsonValue::String(out));
                }
                '\\' => {
                    self.pos += 1;
                    if self.pos >= self.chars.len() {
                        return Err("unterminated escape".to_string());
                    }
                    match self.chars[self.pos] {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => {
                            let mut hex = String::new();
                            for _ in 0..4 {
                                self.pos += 1;
                                if self.pos < self.chars.len() {
                                    hex.push(self.chars[self.pos]);
                                }
                            }
                            if let Ok(code) = u32::from_str_radix(&hex, 16) {
                                if let Some(c) = char::from_u32(code) {
                                    out.push(c);
                                }
                            }
                        }
                        c => out.push(c),
                    }
                    self.pos += 1;
                }
                c => {
                    out.push(c);
                    self.pos += 1;
                }
            }
        }
        Err("unterminated string".to_string())
    }

    fn parse_object(&mut self) -> Result<JsonValue, String> {
        self.pos += 1; // skip {
        let mut items = Vec::new();
        self.skip_ws();
        if self.pos < self.chars.len() && self.chars[self.pos] == '}' {
            self.pos += 1;
            return Ok(JsonValue::Object(items));
        }
        loop {
            self.skip_ws();
            if self.pos >= self.chars.len() || self.chars[self.pos] != '"' {
                return Err("expected key string".to_string());
            }
            let key = match self.parse_string()? {
                JsonValue::String(s) => s,
                _ => return Err("key must be string".to_string()),
            };
            self.skip_ws();
            if self.pos >= self.chars.len() || self.chars[self.pos] != ':' {
                return Err("expected colon".to_string());
            }
            self.pos += 1;
            let val = self.parse_value()?;
            items.push((key, val));
            self.skip_ws();
            if self.pos >= self.chars.len() {
                return Err("unterminated object".to_string());
            }
            match self.chars[self.pos] {
                ',' => {
                    self.pos += 1;
                }
                '}' => {
                    self.pos += 1;
                    return Ok(JsonValue::Object(items));
                }
                _ => return Err(format!("expected , or }}: got {}", self.chars[self.pos])),
            }
        }
    }

    fn parse_array(&mut self) -> Result<JsonValue, String> {
        self.pos += 1; // skip [
        let mut items = Vec::new();
        self.skip_ws();
        if self.pos < self.chars.len() && self.chars[self.pos] == ']' {
            self.pos += 1;
            return Ok(JsonValue::Array(items));
        }
        loop {
            let val = self.parse_value()?;
            items.push(val);
            self.skip_ws();
            if self.pos >= self.chars.len() {
                return Err("unterminated array".to_string());
            }
            match self.chars[self.pos] {
                ',' => {
                    self.pos += 1;
                }
                ']' => {
                    self.pos += 1;
                    return Ok(JsonValue::Array(items));
                }
                _ => return Err(format!("expected , or ]: got {}", self.chars[self.pos])),
            }
        }
    }

    fn parse_bool(&mut self) -> Result<JsonValue, String> {
        if self.chars[self.pos..].starts_with(&['t', 'r', 'u', 'e']) {
            self.pos += 4;
            Ok(JsonValue::Bool(true))
        } else if self.chars[self.pos..].starts_with(&['f', 'a', 'l', 's', 'e']) {
            self.pos += 5;
            Ok(JsonValue::Bool(false))
        } else {
            Err("invalid bool".to_string())
        }
    }

    fn parse_null(&mut self) -> Result<JsonValue, String> {
        if self.chars[self.pos..].starts_with(&['n', 'u', 'l', 'l']) {
            self.pos += 4;
            Ok(JsonValue::Null)
        } else {
            Err("invalid null".to_string())
        }
    }

    fn parse_number(&mut self) -> Result<JsonValue, String> {
        let start = self.pos;
        if self.chars[self.pos] == '-' {
            self.pos += 1;
        }
        while self.pos < self.chars.len()
            && (self.chars[self.pos].is_ascii_digit()
                || self.chars[self.pos] == '.'
                || self.chars[self.pos] == 'e'
                || self.chars[self.pos] == 'E'
                || self.chars[self.pos] == '+'
                || self.chars[self.pos] == '-')
        {
            self.pos += 1;
        }
        let s: String = self.chars[start..self.pos].iter().collect();
        s.parse::<f64>()
            .map(JsonValue::Number)
            .map_err(|e| format!("invalid number: {}", e))
    }
}

/// 将 JsonValue 数组转为 Vec<String>
pub fn json_array_to_strings(v: &JsonValue) -> Vec<String> {
    match v.as_array() {
        Some(arr) => arr
            .iter()
            .filter_map(|item| item.as_str().map(|s| s.to_string()))
            .collect(),
        None => Vec::new(),
    }
}
