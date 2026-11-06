//! JSON minimal, sans dépendance : analyse, écriture et accès tolérant.
//!
//! Tout ce qui arrive du réseau ou du tableau de bord est une donnée NON fiable :
//! l'accès se fait par `get`/`str`/`num`… qui renvoient une valeur neutre au lieu
//! d'échouer, comme les `isObj(v) && typeof v.x === "string"` du code TypeScript.
//!
//! Les nombres gardent leur texte d'origine : un identifiant TikTok sur 64 bits
//! (« 7351234567890123456 ») n'est jamais arrondi par un passage en `f64`.

use std::fmt::Write as _;

/// Objet JSON : l'ordre des clés est conservé (fichiers de réglages stables,
/// comparaison de deux configurations par leur texte).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Map(Vec<(String, Value)>);

impl Map {
    pub fn new() -> Self {
        Map(Vec::new())
    }
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.0.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    /// Remplace la valeur (même position) ou l'ajoute à la fin.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<Value>) {
        let key = key.into();
        let value = value.into();
        match self.0.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => self.0.push((key, value)),
        }
    }
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        let index = self.0.iter().position(|(k, _)| k == key)?;
        Some(self.0.remove(index).1)
    }
    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.0.iter().map(|(k, v)| (k, v))
    }
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.0.iter().map(|(k, _)| k)
    }
    /// Copie superficielle de `other` par-dessus `self` (comme `{ ...a, ...b }`).
    pub fn merge(&mut self, other: &Map) {
        for (k, v) in other.iter() {
            self.insert(k.clone(), v.clone());
        }
    }
}

impl FromIterator<(String, Value)> for Map {
    fn from_iter<T: IntoIterator<Item = (String, Value)>>(iter: T) -> Self {
        let mut map = Map::new();
        for (k, v) in iter {
            map.insert(k, v);
        }
        map
    }
}

/// Nombre JSON : texte d'origine (ou forme canonique) + valeur flottante.
#[derive(Clone, Debug)]
pub struct Number {
    text: String,
    value: f64,
}

impl PartialEq for Number {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value && (self.value.is_finite() || self.text == other.text)
    }
}

impl Number {
    pub fn from_f64(value: f64) -> Number {
        Number { text: format_f64(value), value }
    }
    pub fn as_f64(&self) -> f64 {
        self.value
    }
    /// Entier exact (texte entier sans exposant ni décimale), même au-delà de 2^53.
    pub fn as_i64(&self) -> Option<i64> {
        if let Ok(v) = self.text.parse::<i64>() {
            return Some(v);
        }
        if self.value.fract() == 0.0 && self.value.abs() < 9.007_199_254_740_992e15 {
            return Some(self.value as i64);
        }
        None
    }
    pub fn as_u64(&self) -> Option<u64> {
        if let Ok(v) = self.text.parse::<u64>() {
            return Some(v);
        }
        self.as_i64().and_then(|v| u64::try_from(v).ok())
    }
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// Écriture d'un nombre comme `JSON.stringify` : entiers sans « .0 », non-finis → `null`.
pub fn format_f64(value: f64) -> String {
    if !value.is_finite() {
        return "null".to_string();
    }
    if value == 0.0 {
        return "0".to_string();
    }
    if value.fract() == 0.0 && value.abs() < 1e21 {
        return format!("{}", value as i128);
    }
    let s = format!("{}", value);
    if s.contains('e') || s.contains('E') {
        return s;
    }
    s
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<Value>),
    Object(Map),
}

static NULL: Value = Value::Null;

impl Value {
    pub fn object() -> Value {
        Value::Object(Map::new())
    }

    // -- Lecture tolérante -----------------------------------------------------

    /// Champ d'un objet, ou `null` (jamais d'erreur).
    pub fn get(&self, key: &str) -> &Value {
        match self {
            Value::Object(map) => map.get(key).unwrap_or(&NULL),
            _ => &NULL,
        }
    }
    /// Élément d'un tableau, ou `null`.
    pub fn at(&self, index: usize) -> &Value {
        match self {
            Value::Array(items) => items.get(index).unwrap_or(&NULL),
            _ => &NULL,
        }
    }
    /// Chemin de clés : `v.path(&["a", "b"])`.
    pub fn path(&self, keys: &[&str]) -> &Value {
        keys.iter().fold(self, |v, k| v.get(k))
    }
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
    pub fn is_object(&self) -> bool {
        matches!(self, Value::Object(_))
    }
    pub fn is_array(&self) -> bool {
        matches!(self, Value::Array(_))
    }
    pub fn is_string(&self) -> bool {
        matches!(self, Value::String(_))
    }
    pub fn is_number(&self) -> bool {
        matches!(self, Value::Number(_))
    }
    pub fn as_object(&self) -> Option<&Map> {
        match self {
            Value::Object(map) => Some(map),
            _ => None,
        }
    }
    pub fn as_object_mut(&mut self) -> Option<&mut Map> {
        match self {
            Value::Object(map) => Some(map),
            _ => None,
        }
    }
    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }
    /// Tableau (vide si ce n'en est pas un).
    pub fn items(&self) -> &[Value] {
        match self {
            Value::Array(items) => items,
            _ => &[],
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
    /// Chaîne non vide après `trim`.
    pub fn non_empty_str(&self) -> Option<&str> {
        self.as_str().filter(|s| !s.trim().is_empty())
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
    /// Nombre fini.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(n) if n.value.is_finite() => Some(n.value),
            _ => None,
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Number(n) => n.as_i64(),
            _ => None,
        }
    }
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Number(n) => n.as_u64(),
            _ => None,
        }
    }
    /// Texte d'un identifiant (chaîne, ou nombre entier gardé exact).
    pub fn id_string(&self) -> Option<String> {
        match self {
            Value::String(s) if !s.is_empty() => Some(s.clone()),
            Value::Number(n) => Some(n.text.clone()),
            _ => None,
        }
    }

    // -- Écriture --------------------------------------------------------------

    /// Ajoute/remplace un champ (transforme `self` en objet s'il ne l'est pas).
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<Value>) {
        if !self.is_object() {
            *self = Value::object();
        }
        if let Value::Object(map) = self {
            map.insert(key, value);
        }
    }
    pub fn with(mut self, key: impl Into<String>, value: impl Into<Value>) -> Value {
        self.set(key, value);
        self
    }

    /// Texte JSON compact (identique à `JSON.stringify`).
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        write_value(&mut out, self, None, 0);
        out
    }
    /// Texte JSON indenté (fichiers de réglages lisibles).
    pub fn to_json_pretty(&self) -> String {
        let mut out = String::new();
        write_value(&mut out, self, Some(2), 0);
        out
    }
}

// -- Conversions -------------------------------------------------------------

impl From<bool> for Value {
    fn from(v: bool) -> Value {
        Value::Bool(v)
    }
}
impl From<&str> for Value {
    fn from(v: &str) -> Value {
        Value::String(v.to_string())
    }
}
impl From<String> for Value {
    fn from(v: String) -> Value {
        Value::String(v)
    }
}
impl From<&String> for Value {
    fn from(v: &String) -> Value {
        Value::String(v.clone())
    }
}
impl From<f64> for Value {
    fn from(v: f64) -> Value {
        Value::Number(Number::from_f64(v))
    }
}
macro_rules! from_int {
    ($($t:ty),*) => {$(
        impl From<$t> for Value {
            fn from(v: $t) -> Value {
                Value::Number(Number { text: v.to_string(), value: v as f64 })
            }
        }
    )*};
}
from_int!(i32, i64, u8, u16, u32, u64, usize, isize);
impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(v: Option<T>) -> Value {
        match v {
            Some(v) => v.into(),
            None => Value::Null,
        }
    }
}
impl<T: Into<Value>> From<Vec<T>> for Value {
    fn from(v: Vec<T>) -> Value {
        Value::Array(v.into_iter().map(Into::into).collect())
    }
}
impl From<Map> for Value {
    fn from(v: Map) -> Value {
        Value::Object(v)
    }
}

/// Construit un objet JSON : `obj! { "a" => 1, "b" => "x" }`.
#[macro_export]
macro_rules! obj {
    () => { $crate::json::Value::object() };
    ($($key:expr => $value:expr),+ $(,)?) => {{
        let mut map = $crate::json::Map::new();
        $( map.insert($key, $crate::json::Value::from($value)); )+
        $crate::json::Value::Object(map)
    }};
}

/// Construit un tableau JSON : `arr![1, "x"]`.
#[macro_export]
macro_rules! arr {
    () => { $crate::json::Value::Array(Vec::new()) };
    ($($value:expr),+ $(,)?) => {
        $crate::json::Value::Array(vec![$($crate::json::Value::from($value)),+])
    };
}

// -- Écriture ------------------------------------------------------------------

fn write_value(out: &mut String, value: &Value, indent: Option<usize>, depth: usize) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) => {
            if n.value.is_finite() {
                out.push_str(&n.text)
            } else {
                out.push_str("null")
            }
        }
        Value::String(s) => write_string(out, s),
        Value::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, indent, depth + 1);
                write_value(out, item, indent, depth + 1);
            }
            newline(out, indent, depth);
            out.push(']');
        }
        Value::Object(map) => {
            if map.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (i, (k, v)) in map.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, indent, depth + 1);
                write_string(out, k);
                out.push(':');
                if indent.is_some() {
                    out.push(' ');
                }
                write_value(out, v, indent, depth + 1);
            }
            newline(out, indent, depth);
            out.push('}');
        }
    }
}

fn newline(out: &mut String, indent: Option<usize>, depth: usize) {
    if let Some(width) = indent {
        out.push('\n');
        for _ in 0..width * depth {
            out.push(' ');
        }
    }
}

/// Chaîne JSON échappée comme `JSON.stringify` (non-ASCII gardé tel quel).
pub fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            // U+2028/2029 : valides en JSON mais pas dans un <script> inline.
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn quote(s: &str) -> String {
    let mut out = String::new();
    write_string(&mut out, s);
    out
}

// -- Analyse -------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub offset: usize,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JSON invalide ({} à la position {})", self.message, self.offset)
    }
}
impl std::error::Error for ParseError {}

/// Profondeur maximale : un JSON malveillant très imbriqué ne fait pas déborder la pile.
const MAX_DEPTH: usize = 256;

pub fn parse(text: &str) -> Result<Value, ParseError> {
    let mut p = Parser { s: text.as_bytes(), i: 0, text };
    p.skip_ws();
    // BOM UTF-8 éventuel en tête de fichier.
    if p.s[p.i..].starts_with("\u{feff}".as_bytes()) {
        p.i += 3;
        p.skip_ws();
    }
    let v = p.value(0)?;
    p.skip_ws();
    if p.i != p.s.len() {
        return Err(p.err("caractères en trop"));
    }
    Ok(v)
}

/// Analyse d'octets (UTF-8 attendu ; refusé sinon).
pub fn parse_bytes(bytes: &[u8]) -> Result<Value, ParseError> {
    match std::str::from_utf8(bytes) {
        Ok(text) => parse(text),
        Err(e) => Err(ParseError { message: "UTF-8 invalide".into(), offset: e.valid_up_to() }),
    }
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    text: &'a str,
}

impl<'a> Parser<'a> {
    fn err(&self, message: &str) -> ParseError {
        ParseError { message: message.to_string(), offset: self.i }
    }
    fn skip_ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }
    fn expect(&mut self, word: &str) -> Result<(), ParseError> {
        if self.s[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Ok(())
        } else {
            Err(self.err("mot inattendu"))
        }
    }
    fn value(&mut self, depth: usize) -> Result<Value, ParseError> {
        if depth > MAX_DEPTH {
            return Err(self.err("trop imbriqué"));
        }
        self.skip_ws();
        match self.peek() {
            None => Err(self.err("fin inattendue")),
            Some(b'n') => self.expect("null").map(|_| Value::Null),
            Some(b't') => self.expect("true").map(|_| Value::Bool(true)),
            Some(b'f') => self.expect("false").map(|_| Value::Bool(false)),
            Some(b'"') => self.string().map(Value::String),
            Some(b'[') => {
                self.i += 1;
                let mut items = Vec::new();
                self.skip_ws();
                if self.peek() == Some(b']') {
                    self.i += 1;
                    return Ok(Value::Array(items));
                }
                loop {
                    items.push(self.value(depth + 1)?);
                    self.skip_ws();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Value::Array(items));
                        }
                        _ => return Err(self.err("« , » ou « ] » attendu")),
                    }
                }
            }
            Some(b'{') => {
                self.i += 1;
                let mut map = Map::new();
                self.skip_ws();
                if self.peek() == Some(b'}') {
                    self.i += 1;
                    return Ok(Value::Object(map));
                }
                loop {
                    self.skip_ws();
                    if self.peek() != Some(b'"') {
                        return Err(self.err("clé attendue"));
                    }
                    let key = self.string()?;
                    self.skip_ws();
                    if self.peek() != Some(b':') {
                        return Err(self.err("« : » attendu"));
                    }
                    self.i += 1;
                    let v = self.value(depth + 1)?;
                    map.insert(key, v);
                    self.skip_ws();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Value::Object(map));
                        }
                        _ => return Err(self.err("« , » ou « } » attendu")),
                    }
                }
            }
            Some(c) if c == b'-' || c.is_ascii_digit() => self.number(),
            Some(_) => Err(self.err("caractère inattendu")),
        }
    }
    fn number(&mut self) -> Result<Value, ParseError> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        match self.peek() {
            Some(b'0') => self.i += 1,
            Some(c) if c.is_ascii_digit() => {
                while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                    self.i += 1;
                }
            }
            _ => return Err(self.err("nombre invalide")),
        }
        if self.peek() == Some(b'.') {
            self.i += 1;
            if !matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                return Err(self.err("nombre invalide"));
            }
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.i += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if !matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                return Err(self.err("nombre invalide"));
            }
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.i += 1;
            }
        }
        let text = &self.text[start..self.i];
        let value: f64 = text.parse().map_err(|_| self.err("nombre invalide"))?;
        Ok(Value::Number(Number { text: text.to_string(), value }))
    }
    fn hex4(&mut self) -> Result<u32, ParseError> {
        if self.i + 4 > self.s.len() {
            return Err(self.err("échappement \\u incomplet"));
        }
        let h = std::str::from_utf8(&self.s[self.i..self.i + 4]).map_err(|_| self.err("échappement invalide"))?;
        let v = u32::from_str_radix(h, 16).map_err(|_| self.err("échappement invalide"))?;
        self.i += 4;
        Ok(v)
    }
    fn string(&mut self) -> Result<String, ParseError> {
        self.i += 1; // guillemet ouvrant
        let mut out = String::new();
        loop {
            let start = self.i;
            while self.i < self.s.len() && self.s[self.i] != b'"' && self.s[self.i] != b'\\' && self.s[self.i] >= 0x20 {
                self.i += 1;
            }
            out.push_str(&self.text[start..self.i]);
            match self.peek() {
                None => return Err(self.err("chaîne non terminée")),
                Some(b'"') => {
                    self.i += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.i += 1;
                    let c = self.peek().ok_or_else(|| self.err("échappement incomplet"))?;
                    self.i += 1;
                    match c {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{08}'),
                        b'f' => out.push('\u{0c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let first = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&first) {
                                // Paire de substitution : le second demi-caractère doit suivre.
                                if self.s[self.i..].starts_with(b"\\u") {
                                    let save = self.i;
                                    self.i += 2;
                                    let second = self.hex4()?;
                                    if (0xDC00..0xE000).contains(&second) {
                                        0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                                    } else {
                                        self.i = save;
                                        0xFFFD
                                    }
                                } else {
                                    0xFFFD
                                }
                            } else if (0xDC00..0xE000).contains(&first) {
                                0xFFFD
                            } else {
                                first
                            };
                            out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                        }
                        _ => return Err(self.err("échappement inconnu")),
                    }
                }
                Some(_) => return Err(self.err("caractère de contrôle dans une chaîne")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_like_json_stringify() {
        let text = r#"{"a":1,"b":[true,false,null],"c":"é\"\\\n","d":{"e":-0.5,"f":1e3}}"#;
        let v = parse(text).unwrap();
        assert_eq!(v.get("a").as_f64(), Some(1.0));
        assert_eq!(v.get("c").as_str(), Some("é\"\\\n"));
        assert_eq!(v.path(&["d", "e"]).as_f64(), Some(-0.5));
        assert_eq!(v.to_json(), text);
    }

    #[test]
    fn big_ids_stay_exact() {
        let v = parse(r#"{"roomId":7351234567890123456,"x":12}"#).unwrap();
        assert_eq!(v.get("roomId").id_string().as_deref(), Some("7351234567890123456"));
        assert_eq!(v.get("roomId").as_u64(), Some(7351234567890123456));
        assert_eq!(v.to_json(), r#"{"roomId":7351234567890123456,"x":12}"#);
    }

    #[test]
    fn numbers_written_like_js() {
        assert_eq!(Value::from(3.0).to_json(), "3");
        assert_eq!(Value::from(0.6).to_json(), "0.6");
        assert_eq!(Value::from(f64::NAN).to_json(), "null");
        assert_eq!(Value::from(-0.0).to_json(), "0");
        assert_eq!(Value::from(1234567u64).to_json(), "1234567");
    }

    #[test]
    fn surrogates_and_controls() {
        let v = parse(r#""😀 é \ud800x""#).unwrap();
        assert_eq!(v.as_str(), Some("😀 é \u{FFFD}x"));
        assert_eq!(Value::from("a\u{01}b\u{2028}").to_json(), "\"a\\u0001b\\u2028\"");
    }

    #[test]
    fn rejects_garbage_without_panicking() {
        for bad in ["", "{", "[1,]", "{\"a\" 1}", "01", "1.", "\"\u{01}\"", "nul", "{} x", "-", "1e"] {
            assert!(parse(bad).is_err(), "{bad:?} devrait être refusé");
        }
        let deep = "[".repeat(1000) + &"]".repeat(1000);
        assert!(parse(&deep).is_err());
    }

    #[test]
    fn tolerant_access() {
        let v = parse(r#"{"a":{"b":"x"},"n":"12"}"#).unwrap();
        assert!(v.path(&["a", "z", "q"]).is_null());
        assert_eq!(v.get("n").as_f64(), None);
        assert!(v.at(3).is_null());
        assert_eq!(v.get("a").get("b").non_empty_str(), Some("x"));
    }

    #[test]
    fn macros_and_order() {
        let mut v = obj! { "b" => 1, "a" => "x" };
        v.set("b", 2);
        v.set("c", arr![1, "y", Value::Null]);
        assert_eq!(v.to_json(), r#"{"b":2,"a":"x","c":[1,"y",null]}"#);
        assert_eq!(v.to_json_pretty(), "{\n  \"b\": 2,\n  \"a\": \"x\",\n  \"c\": [\n    1,\n    \"y\",\n    null\n  ]\n}");
    }

    #[test]
    fn bom_is_ignored() {
        assert_eq!(parse("\u{feff}{\"a\":1}").unwrap().get("a").as_f64(), Some(1.0));
    }
}
