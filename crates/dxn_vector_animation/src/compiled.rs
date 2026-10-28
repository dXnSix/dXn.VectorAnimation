//! The compiled scene format (`.vector-animation.json`): a scene saved as
//! JSON with its format name and version, so a player loads it without
//! reading SVG or Lottie again.

use std::fmt::{self, Write as _};

use serde::{Deserialize, Serialize, ser};

use crate::{Error, FORMAT, FORMAT_VERSION, Limits, Scene};

#[derive(serde::Serialize)]
struct Out<'a> {
    format: &'static str,
    version: u32,
    scene: &'a Scene,
}

/// What a compiled scene says it is, read before its scene.
#[derive(serde::Deserialize)]
struct Head {
    format: String,
    version: u32,
}

#[derive(serde::Deserialize)]
struct In {
    format: String,
    version: u32,
    scene: Scene,
}

/// `scene` in the compiled format. A scene holding a number JSON cannot
/// hold (NaN or infinite) is refused with [`Error::Unsupported`], which
/// says where the number is.
pub fn to_json(scene: &Scene, pretty: bool) -> Result<String, Error> {
    let out = Out {
        format: FORMAT,
        version: FORMAT_VERSION,
        scene,
    };
    // serde_json would write such a number as `null`, which no scene reads.
    out.serialize(&mut Finite::default())
        .map_err(|e| Error::Unsupported(e.0))?;
    let text = if pretty {
        serde_json::to_string_pretty(&out)
    } else {
        serde_json::to_string(&out)
    };
    text.map_err(|e| Error::Unsupported(e.to_string()))
}

/// Whether `data` is a compiled scene.
pub fn sniff(data: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&data[..data.len().min(256)]);
    head.trim_start().starts_with('{') && head.contains(FORMAT)
}

/// Reads a compiled scene, checked against `limits`.
pub fn from_json(data: &[u8], limits: &Limits) -> Result<Scene, Error> {
    if data.len() > limits.max_bytes {
        return Err(Error::Limit {
            what: "bytes",
            limit: limits.max_bytes as f64,
        });
    }
    read_in(
        || serde_json::from_slice(data),
        || serde_json::from_slice(data),
        limits,
    )
}

/// Reads a compiled scene from its JSON value (an app's import of the
/// file), checked against `limits`: as [`from_json`] reads its text.
pub fn from_value(value: &serde_json::Value, limits: &Limits) -> Result<Scene, Error> {
    read_in(
        || In::deserialize(value),
        || Head::deserialize(value),
        limits,
    )
}

/// The scene `parse` reads, its format and version checked (by `head` where
/// it does not read: a newer file's scene may not parse).
fn read_in(
    parse: impl FnOnce() -> Result<In, serde_json::Error>,
    head: impl FnOnce() -> Result<Head, serde_json::Error>,
    limits: &Limits,
) -> Result<Scene, Error> {
    let invalid = |e: serde_json::Error| Error::Invalid(e.to_string());
    let checked = |head: Head| {
        if head.format != FORMAT {
            return Err(Error::Invalid(format!("not a {FORMAT} file")));
        }
        if head.version > FORMAT_VERSION {
            return Err(Error::Unsupported(format!(
                "format version {} (this library reads up to {FORMAT_VERSION})",
                head.version
            )));
        }
        Ok(())
    };
    // Read once; where it does not read, the format and version say why
    // first.
    let parsed = match parse() {
        Ok(parsed) => parsed,
        Err(e) => {
            checked(head().map_err(invalid)?)?;
            return Err(invalid(e));
        }
    };
    checked(Head {
        format: parsed.format,
        version: parsed.version,
    })?;
    parsed.scene.check(limits)?;
    Ok(parsed.scene)
}

/// Walks a value as it is serialized for the first number that is not
/// finite, keeping the way there (field names as the JSON writes them).
#[derive(Default)]
struct Finite {
    path: Vec<Step>,
}

enum Step {
    Field(&'static str),
    Index(usize),
}

/// Where a number that is not finite is, and what it is.
#[derive(Debug)]
struct NotFinite(String);

impl fmt::Display for NotFinite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NotFinite {}

impl ser::Error for NotFinite {
    fn custom<T: fmt::Display>(message: T) -> Self {
        NotFinite(message.to_string())
    }
}

type Done = Result<(), NotFinite>;

impl Finite {
    fn number(&self, value: f64) -> Done {
        if value.is_finite() {
            return Ok(());
        }
        let mut at = String::new();
        for step in &self.path {
            match step {
                Step::Field(name) if at.is_empty() => at.push_str(name),
                Step::Field(name) => {
                    at.push('.');
                    at.push_str(name);
                }
                Step::Index(i) => {
                    let _ = write!(at, "[{i}]");
                }
            }
        }
        Err(NotFinite(format!(
            "`{at}` is {value}, which the compiled format cannot hold"
        )))
    }

    fn inside<T: ?Sized + Serialize>(&mut self, step: Step, value: &T) -> Done {
        self.path.push(step);
        value.serialize(&mut *self)?;
        self.path.pop();
        Ok(())
    }
}

/// The items of a list, tuple or tuple variant.
struct Items<'a> {
    finite: &'a mut Finite,
    index: usize,
    /// Whether a variant's name is on the path, to take off at the end.
    variant: bool,
}

impl Items<'_> {
    fn item<T: ?Sized + Serialize>(&mut self, value: &T) -> Done {
        self.finite.inside(Step::Index(self.index), value)?;
        self.index += 1;
        Ok(())
    }

    fn end(self) -> Done {
        if self.variant {
            self.finite.path.pop();
        }
        Ok(())
    }
}

/// The fields of a struct or struct variant, or the entries of a map.
struct Fields<'a> {
    finite: &'a mut Finite,
    variant: bool,
}

impl Fields<'_> {
    fn end(self) -> Done {
        if self.variant {
            self.finite.path.pop();
        }
        Ok(())
    }
}

impl<'a> ser::Serializer for &'a mut Finite {
    type Ok = ();
    type Error = NotFinite;
    type SerializeSeq = Items<'a>;
    type SerializeTuple = Items<'a>;
    type SerializeTupleStruct = Items<'a>;
    type SerializeTupleVariant = Items<'a>;
    type SerializeMap = Fields<'a>;
    type SerializeStruct = Fields<'a>;
    type SerializeStructVariant = Fields<'a>;

    fn serialize_bool(self, _: bool) -> Done {
        Ok(())
    }
    fn serialize_i8(self, _: i8) -> Done {
        Ok(())
    }
    fn serialize_i16(self, _: i16) -> Done {
        Ok(())
    }
    fn serialize_i32(self, _: i32) -> Done {
        Ok(())
    }
    fn serialize_i64(self, _: i64) -> Done {
        Ok(())
    }
    fn serialize_u8(self, _: u8) -> Done {
        Ok(())
    }
    fn serialize_u16(self, _: u16) -> Done {
        Ok(())
    }
    fn serialize_u32(self, _: u32) -> Done {
        Ok(())
    }
    fn serialize_u64(self, _: u64) -> Done {
        Ok(())
    }
    fn serialize_f32(self, value: f32) -> Done {
        self.number(f64::from(value))
    }
    fn serialize_f64(self, value: f64) -> Done {
        self.number(value)
    }
    fn serialize_char(self, _: char) -> Done {
        Ok(())
    }
    fn serialize_str(self, _: &str) -> Done {
        Ok(())
    }
    fn serialize_bytes(self, _: &[u8]) -> Done {
        Ok(())
    }
    fn serialize_none(self) -> Done {
        Ok(())
    }
    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Done {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Done {
        Ok(())
    }
    fn serialize_unit_struct(self, _: &'static str) -> Done {
        Ok(())
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, _: &'static str) -> Done {
        Ok(())
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(self, _: &'static str, value: &T) -> Done {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        value: &T,
    ) -> Done {
        self.inside(Step::Field(variant), value)
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Items<'a>, NotFinite> {
        Ok(Items {
            finite: self,
            index: 0,
            variant: false,
        })
    }
    fn serialize_tuple(self, len: usize) -> Result<Items<'a>, NotFinite> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_struct(self, _: &'static str, len: usize) -> Result<Items<'a>, NotFinite> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Items<'a>, NotFinite> {
        self.path.push(Step::Field(variant));
        Ok(Items {
            finite: self,
            index: 0,
            variant: true,
        })
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Fields<'a>, NotFinite> {
        Ok(Fields {
            finite: self,
            variant: false,
        })
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Fields<'a>, NotFinite> {
        self.serialize_map(None)
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Fields<'a>, NotFinite> {
        self.path.push(Step::Field(variant));
        Ok(Fields {
            finite: self,
            variant: true,
        })
    }
}

impl ser::SerializeSeq for Items<'_> {
    type Ok = ();
    type Error = NotFinite;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Done {
        self.item(value)
    }
    fn end(self) -> Done {
        Items::end(self)
    }
}

impl ser::SerializeTuple for Items<'_> {
    type Ok = ();
    type Error = NotFinite;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Done {
        self.item(value)
    }
    fn end(self) -> Done {
        Items::end(self)
    }
}

impl ser::SerializeTupleStruct for Items<'_> {
    type Ok = ();
    type Error = NotFinite;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Done {
        self.item(value)
    }
    fn end(self) -> Done {
        Items::end(self)
    }
}

impl ser::SerializeTupleVariant for Items<'_> {
    type Ok = ();
    type Error = NotFinite;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Done {
        self.item(value)
    }
    fn end(self) -> Done {
        Items::end(self)
    }
}

impl ser::SerializeMap for Fields<'_> {
    type Ok = ();
    type Error = NotFinite;
    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Done {
        key.serialize(&mut *self.finite)
    }
    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Done {
        value.serialize(&mut *self.finite)
    }
    fn end(self) -> Done {
        Fields::end(self)
    }
}

impl ser::SerializeStruct for Fields<'_> {
    type Ok = ();
    type Error = NotFinite;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, key: &'static str, value: &T) -> Done {
        self.finite.inside(Step::Field(key), value)
    }
    fn end(self) -> Done {
        Fields::end(self)
    }
}

impl ser::SerializeStructVariant for Fields<'_> {
    type Ok = ();
    type Error = NotFinite;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, key: &'static str, value: &T) -> Done {
        self.finite.inside(Step::Field(key), value)
    }
    fn end(self) -> Done {
        Fields::end(self)
    }
}
