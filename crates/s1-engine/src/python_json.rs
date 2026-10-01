use std::io;

use serde::Serialize;
use serde_json::Value;
use serde_json::ser::Formatter;

/// Serializes JSON exactly like Python's `json.dumps(value, ensure_ascii=False)`, which is how Laya
/// turns structured states and criteria into text before tokenizing them.
pub struct PythonJson;

impl PythonJson {
    pub fn dumps(value: &Value) -> String {
        let mut buffer = Vec::new();
        let mut serializer = serde_json::Serializer::with_formatter(&mut buffer, PythonFormatter);
        value
            .serialize(&mut serializer)
            .expect("serializing a JSON value into memory cannot fail");
        String::from_utf8(buffer).expect("serde_json writes valid UTF-8")
    }

    /// A state or criterion as Laya reads it: strings verbatim, anything else as Python JSON.
    pub fn text(value: &Value) -> String {
        match value {
            Value::String(text) => text.clone(),
            other => Self::dumps(other),
        }
    }
}

struct PythonFormatter;

impl PythonFormatter {
    /// Python's float repr: shortest round-trip digits, scientific notation when the decimal
    /// exponent is below -4 or at least 16, a signed two-digit exponent, and ".0" on whole numbers.
    fn float_repr(number: f64) -> String {
        let shortest = format!("{number:e}");
        let (mantissa, exponent) = shortest.split_once('e').expect("{:e} always has an exponent");
        let exponent: i32 = exponent.parse().expect("{:e} exponent is an integer");
        let negative = mantissa.starts_with('-');
        let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
        let sign = if negative { "-" } else { "" };
        if !(-4..16).contains(&exponent) {
            let fraction = &digits[1..];
            let mantissa = if fraction.is_empty() {
                digits[..1].to_string()
            } else {
                format!("{}.{}", &digits[..1], fraction)
            };
            let exponent_sign = if exponent < 0 { '-' } else { '+' };
            return format!("{sign}{mantissa}e{exponent_sign}{:02}", exponent.abs());
        }
        let point = exponent + 1;
        if point <= 0 {
            return format!("{sign}0.{}{digits}", "0".repeat(point.unsigned_abs() as usize));
        }
        let point = point as usize;
        if digits.len() <= point {
            return format!("{sign}{digits}{}.0", "0".repeat(point - digits.len()));
        }
        format!("{sign}{}.{}", &digits[..point], &digits[point..])
    }
}

impl Formatter for PythonFormatter {
    fn begin_array_value<W: ?Sized + io::Write>(&mut self, writer: &mut W, first: bool) -> io::Result<()> {
        if first { Ok(()) } else { writer.write_all(b", ") }
    }

    fn begin_object_key<W: ?Sized + io::Write>(&mut self, writer: &mut W, first: bool) -> io::Result<()> {
        if first { Ok(()) } else { writer.write_all(b", ") }
    }

    fn begin_object_value<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        writer.write_all(b": ")
    }

    fn write_f64<W: ?Sized + io::Write>(&mut self, writer: &mut W, value: f64) -> io::Result<()> {
        writer.write_all(Self::float_repr(value).as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::PythonJson;

    #[test]
    fn matches_python_separators_and_unicode() {
        let value =
            json!({"path": "src/tax.py", "lines": [10, 42], "note": "año ñandú", "nested": {"ok": true, "none": null}});
        assert_eq!(
            PythonJson::dumps(&value),
            r#"{"path": "src/tax.py", "lines": [10, 42], "note": "año ñandú", "nested": {"ok": true, "none": null}}"#
        );
    }

    #[test]
    fn matches_python_float_repr() {
        let cases = [
            (1.0, "1.0"),
            (0.5, "0.5"),
            (1e-5, "1e-05"),
            (0.0001, "0.0001"),
            (1e16, "1e+16"),
            (123456789012345.0, "123456789012345.0"),
            (-2.5e-7, "-2.5e-07"),
            (0.1, "0.1"),
            (1.5e300, "1.5e+300"),
        ];
        for (number, expected) in cases {
            assert_eq!(PythonJson::dumps(&json!(number)), expected, "repr of {number}");
        }
    }

    #[test]
    fn escapes_like_python() {
        assert_eq!(PythonJson::dumps(&json!("a\"b\\c\nd\u{1}")), r#""a\"b\\c\nd\u0001""#);
    }
}
