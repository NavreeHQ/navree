//! `native-models [--check]`: writes the Kotlin, Swift and C# models of Navree
//! Native's protocol from its JSON Schema, or says which written files are
//! stale.
//!
//! The schema (`crates/navree-native/schema/navree-native.schema.json`) is
//! written by navree-native's own tests from its Rust types, so the models
//! follow the types through it and a client never writes a model by hand. The
//! output is plain and regular on purpose: each definition is written the same
//! way, so a pattern right once is right everywhere.
//!
//! What both models do, as the Rust types do:
//! - a field a value may leave out holds its default when it is absent, and is
//!   left out when it holds its default;
//! - a field the model does not know is passed over;
//! - a union's variant it does not know is read as `Unknown`, which keeps its
//!   tag and what the schema says it keeps (a part's `id` and `alt`);
//! - a closed-list value it does not know, in a field that has a default, is
//!   read as that default.
//!
//! Kotlin: one `object Navree` holding `@Serializable` data classes, enum
//! classes, and for each union a `sealed interface` with a serializer of its
//! own that picks the variant by its tag (kotlinx.serialization, JSON only).
//! Swift: one `enum Navree` namespace holding `Codable`, `Hashable`, `Sendable`
//! structs with their own coding, string enums, and an `indirect enum` for
//! each union.
//! C#: one `static partial class Models` holding sealed records, enums with an
//! `Unknown` member for a word the build does not know, and an interface for
//! each union (a record can stand in more than one). Each definition has a
//! `Read` that takes a `JsonElement` and a `Write` that takes a
//! `Utf8JsonWriter`, written out plainly: no reflection, so nothing is lost to
//! trimming, and nothing in the rules depends on the order of a value's keys.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// The schema, relative to the workspace root.
pub const SCHEMA: &str = "crates/navree-native/schema/navree-native.schema.json";

/// The Kotlin models, relative to the workspace root.
pub const KOTLIN: &str =
    "packages/android/navree-native/src/main/kotlin/com/navree/protocol/Models.kt";

/// The Swift models, relative to the workspace root.
pub const SWIFT: &str = "packages/apple/Sources/NavreeProtocol/Models.swift";

/// The C# models, relative to the workspace root.
pub const CSHARP: &str = "packages/windows/src/Navree.Protocol/Models.cs";

/// The models written from one schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Models {
    /// `Models.kt`.
    pub kotlin: String,
    /// `Models.swift`.
    pub swift: String,
    /// `Models.cs`.
    pub csharp: String,
}

/// Writes the models from the schema at `root`, or with `check` writes
/// nothing and returns the files that are missing or differ from what would
/// be written.
pub fn run(root: &Path, check: bool) -> Result<Vec<PathBuf>, String> {
    let schema_path = root.join(SCHEMA);
    let text = std::fs::read_to_string(&schema_path)
        .map_err(|error| format!("{}: {error}", schema_path.display()))?;
    let schema: Value = serde_json::from_str(&text)
        .map_err(|error| format!("{}: {error}", schema_path.display()))?;
    let models = generate(&schema)?;
    let mut stale = Vec::new();
    for (path, written) in [
        (KOTLIN, &models.kotlin),
        (SWIFT, &models.swift),
        (CSHARP, &models.csharp),
    ] {
        let path = root.join(path);
        let on_disk = std::fs::read_to_string(&path)
            .map(|text| text.replace("\r\n", "\n"))
            .ok();
        if on_disk.as_deref() == Some(written.as_str()) {
            continue;
        }
        if check {
            stale.push(path);
        } else {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|error| format!("{}: {error}", parent.display()))?;
            }
            std::fs::write(&path, written)
                .map_err(|error| format!("{}: {error}", path.display()))?;
        }
    }
    Ok(stale)
}

/// The shape of a field.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Ty {
    Text,
    Bool,
    /// An integer that fits 32 bits.
    Int,
    /// An integer that may not.
    Long,
    Double,
    /// Any JSON value.
    Json,
    List(Box<Ty>),
    Map(Box<Ty>),
    Optional(Box<Ty>),
    Ref(String),
}

#[derive(Debug, Clone)]
struct Field {
    wire: String,
    ty: Ty,
    /// `None` for a field every value holds.
    default: Option<Value>,
}

#[derive(Debug, Clone)]
struct Variant {
    tag: String,
    payload: Option<String>,
}

#[derive(Debug, Clone)]
enum Def {
    Object(Vec<Field>),
    Enum(Vec<String>),
    Union {
        tag: String,
        variants: Vec<Variant>,
        keeps: Vec<(String, Ty)>,
    },
    /// Text with a rule of its own: a string in both languages.
    Text,
}

/// The definitions of a schema, read into the shapes above.
struct Read {
    defs: BTreeMap<String, Def>,
    major: u64,
    vocab: u64,
}

fn ty(shape: &Value, at: &str) -> Result<Ty, String> {
    if let Some(path) = shape.get("$ref").and_then(Value::as_str) {
        let name = path
            .strip_prefix("#/$defs/")
            .ok_or_else(|| format!("{at}: a reference outside the definitions: {path}"))?;
        return Ok(Ty::Ref(name.to_owned()));
    }
    if let Some(Value::Array(any)) = shape.get("anyOf") {
        let inner = any
            .iter()
            .find(|option| option.get("type").and_then(Value::as_str) != Some("null"))
            .ok_or_else(|| format!("{at}: an `anyOf` of nothing but null"))?;
        return Ok(Ty::Optional(Box::new(ty(inner, at)?)));
    }
    if shape.get("x-json").is_some() {
        return Ok(Ty::Json);
    }
    match (
        shape.get("type").and_then(Value::as_str),
        shape.get("format").and_then(Value::as_str),
    ) {
        (Some("string"), _) => Ok(Ty::Text),
        (Some("boolean"), _) => Ok(Ty::Bool),
        (Some("integer"), Some("uint8" | "uint16")) => Ok(Ty::Int),
        (Some("integer"), _) => Ok(Ty::Long),
        (Some("number"), _) => Ok(Ty::Double),
        (Some("array"), _) => Ok(Ty::List(Box::new(ty(&shape["items"], at)?))),
        (Some("object"), _) => Ok(Ty::Map(Box::new(ty(&shape["additionalProperties"], at)?))),
        _ => Err(format!(
            "{at}: a shape the generator does not know: {shape}"
        )),
    }
}

fn read(schema: &Value) -> Result<Read, String> {
    let defs = schema["$defs"]
        .as_object()
        .ok_or("the schema has no `$defs`")?;
    let mut out = BTreeMap::new();
    for (name, def) in defs {
        let kind = def["x-kind"].as_str().unwrap_or_default();
        let read = match kind {
            "object" => {
                let required: BTreeSet<&str> = def["required"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                let mut fields = Vec::new();
                for (wire, property) in def["properties"].as_object().into_iter().flatten() {
                    let at = format!("{name}.{wire}");
                    let default =
                        if required.contains(wire.as_str()) {
                            None
                        } else {
                            Some(property.get("default").cloned().ok_or_else(|| {
                                format!("{at}: may be left out but has no default")
                            })?)
                        };
                    fields.push(Field {
                        wire: wire.clone(),
                        ty: ty(property, &at)?,
                        default,
                    });
                }
                if fields.is_empty() {
                    return Err(format!("{name}: an object with no fields"));
                }
                Def::Object(fields)
            }
            "enum" => Def::Enum(
                def["enum"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|value| {
                        value
                            .as_str()
                            .map(str::to_owned)
                            .ok_or_else(|| format!("{name}: a value that is not a name"))
                    })
                    .collect::<Result<_, _>>()?,
            ),
            "union" => {
                let tag = def["x-tag"]
                    .as_str()
                    .ok_or_else(|| format!("{name}: a union with no `x-tag`"))?
                    .to_owned();
                let variants = def["x-variants"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|variant| {
                        Ok(Variant {
                            tag: variant["tag"]
                                .as_str()
                                .ok_or_else(|| format!("{name}: a variant with no tag"))?
                                .to_owned(),
                            payload: variant["type"].as_str().map(str::to_owned),
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                let mut keeps = Vec::new();
                for (field, shape) in def["x-unknown"].as_object().into_iter().flatten() {
                    keeps.push((
                        field.clone(),
                        ty(shape, &format!("{name}.x-unknown.{field}"))?,
                    ));
                }
                Def::Union {
                    tag,
                    variants,
                    keeps,
                }
            }
            "string" => Def::Text,
            _ => {
                return Err(format!(
                    "{name}: a definition of no kind the generator knows"
                ));
            }
        };
        out.insert(name.clone(), read);
    }
    let read = Read {
        defs: out,
        major: schema["x-major"]
            .as_u64()
            .ok_or("the schema has no `x-major`")?,
        vocab: schema["x-vocab"]
            .as_u64()
            .ok_or("the schema has no `x-vocab`")?,
    };
    read.check()?;
    Ok(read)
}

impl Read {
    /// Refuses what one of the languages could not hold: a reference to a
    /// definition that is not there, a struct that holds itself with nothing
    /// between (which has no size in Swift), and a union variant that holds
    /// something other than an object.
    fn check(&self) -> Result<(), String> {
        let mut refs = Vec::new();
        for (name, def) in &self.defs {
            match def {
                Def::Object(fields) => {
                    for field in fields {
                        refs.push((format!("{name}.{}", field.wire), field.ty.clone()));
                    }
                }
                Def::Union {
                    variants, keeps, ..
                } => {
                    for variant in variants {
                        if let Some(payload) = &variant.payload {
                            match self.defs.get(payload) {
                                Some(Def::Object(_)) => {}
                                _ => {
                                    return Err(format!(
                                        "{name}.{}: holds `{payload}`, which is not an object",
                                        variant.tag
                                    ));
                                }
                            }
                        }
                    }
                    for (field, ty) in keeps {
                        refs.push((format!("{name}.x-unknown.{field}"), ty.clone()));
                    }
                }
                Def::Enum(_) | Def::Text => {}
            }
        }
        for (at, ty) in refs {
            let mut ty = &ty;
            while let Ty::List(inner) | Ty::Map(inner) | Ty::Optional(inner) = ty {
                ty = inner;
            }
            if let Ty::Ref(name) = ty
                && !self.defs.contains_key(name)
            {
                return Err(format!("{at}: refers to `{name}`, which is not defined"));
            }
        }
        // A struct that holds a struct, directly or as an optional value.
        let held = |name: &str| -> Vec<String> {
            let Some(Def::Object(fields)) = self.defs.get(name) else {
                return Vec::new();
            };
            fields
                .iter()
                .filter_map(|field| match &field.ty {
                    Ty::Ref(to) => Some(to.clone()),
                    Ty::Optional(inner) => match inner.as_ref() {
                        Ty::Ref(to) => Some(to.clone()),
                        _ => None,
                    },
                    _ => None,
                })
                .filter(|to| matches!(self.defs.get(to), Some(Def::Object(_))))
                .collect()
        };
        for start in self.defs.keys() {
            let mut seen = BTreeSet::new();
            let mut stack = held(start);
            while let Some(next) = stack.pop() {
                if next == *start {
                    return Err(format!(
                        "{start} holds itself with no list or union between"
                    ));
                }
                if seen.insert(next.clone()) {
                    stack.extend(held(&next));
                }
            }
        }
        Ok(())
    }

    /// The unions each object stands in as a variant on its own, and which
    /// definitions belong to one variant alone (`Union.Variant`).
    fn unions_of(&self, name: &str) -> Vec<&str> {
        self.defs
            .iter()
            .filter_map(|(union, def)| match def {
                Def::Union { variants, .. }
                    if variants
                        .iter()
                        .any(|variant| variant.payload.as_deref() == Some(name)) =>
                {
                    Some(union.as_str())
                }
                _ => None,
            })
            .collect()
    }

    fn is_enum(&self, ty: &Ty) -> bool {
        matches!(ty, Ty::Ref(name) if matches!(self.defs.get(name), Some(Def::Enum(_))))
    }

    /// `value`, when the closed list `name` holds it.
    fn enum_value<'v>(&self, name: &str, value: &'v str, at: &str) -> Result<&'v str, String> {
        match self.defs.get(name) {
            Some(Def::Enum(values)) if values.iter().any(|known| known == value) => Ok(value),
            _ => Err(format!("{at}: the default `{value}` is not a {name}")),
        }
    }
}

/// `view_box` as `viewBox`.
fn camel(wire: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for letter in wire.chars() {
        if letter == '_' || letter == '-' {
            upper = true;
        } else if upper {
            out.push(letter.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(letter);
        }
    }
    out
}

/// `large_title` as `LARGE_TITLE`.
fn screaming(wire: &str) -> String {
    wire.replace('-', "_").to_ascii_uppercase()
}

/// A definition's own name, without the union it belongs to.
fn last(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

fn json_string(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_default()
}

const HEADER: &str = "Generated by `cargo xtask native-models` from \
                      crates/navree-native/schema/navree-native.schema.json. Do not edit: change \
                      the Rust types in crates/navree-native, then run the command again.";

/// Writes the models from `schema`.
pub fn generate(schema: &Value) -> Result<Models, String> {
    let read = read(schema)?;
    Ok(Models {
        kotlin: Kotlin(&read).file()?,
        swift: Swift(&read).file()?,
        csharp: CSharp(&read).file()?,
    })
}

struct Kotlin<'a>(&'a Read);

const KOTLIN_KEYWORDS: &[&str] = &[
    "as",
    "break",
    "class",
    "continue",
    "do",
    "else",
    "false",
    "for",
    "fun",
    "if",
    "in",
    "interface",
    "is",
    "null",
    "object",
    "package",
    "return",
    "super",
    "this",
    "throw",
    "true",
    "try",
    "typealias",
    "typeof",
    "val",
    "var",
    "when",
    "while",
];

impl Kotlin<'_> {
    fn property(wire: &str) -> String {
        let name = camel(wire);
        if KOTLIN_KEYWORDS.contains(&name.as_str()) {
            format!("`{name}`")
        } else {
            name
        }
    }

    fn type_name(&self, name: &str) -> String {
        match self.0.defs.get(name) {
            Some(Def::Text) => "String".to_owned(),
            _ => format!("Navree.{name}"),
        }
    }

    fn ty(&self, ty: &Ty) -> String {
        match ty {
            Ty::Text => "String".to_owned(),
            Ty::Bool => "Boolean".to_owned(),
            Ty::Int => "Int".to_owned(),
            Ty::Long => "Long".to_owned(),
            Ty::Double => "Double".to_owned(),
            Ty::Json => "JsonElement".to_owned(),
            Ty::List(item) => format!("kotlin.collections.List<{}>", self.ty(item)),
            Ty::Map(value) => format!("kotlin.collections.Map<String, {}>", self.ty(value)),
            Ty::Optional(inner) => format!("{}?", self.ty(inner)),
            Ty::Ref(name) => self.type_name(name),
        }
    }

    /// The field's type as declared: nullable when its default is `null`.
    fn field_ty(&self, field: &Field) -> String {
        match (&field.ty, &field.default) {
            (Ty::Json, Some(Value::Null)) => "JsonElement?".to_owned(),
            (ty, _) => self.ty(ty),
        }
    }

    fn default(&self, field: &Field, default: &Value, at: &str) -> Result<String, String> {
        let ty = match &field.ty {
            Ty::Optional(_) | Ty::Json if default.is_null() => return Ok("null".to_owned()),
            ty => ty,
        };
        Ok(match (ty, default) {
            (Ty::Bool, Value::Bool(value)) => value.to_string(),
            (Ty::Int, Value::Number(value)) => value.to_string(),
            (Ty::Long, Value::Number(value)) => format!("{value}L"),
            (Ty::Double, Value::Number(value)) => {
                format!("{:?}", value.as_f64().unwrap_or_default())
            }
            (Ty::Text, Value::String(text)) => json_string(text).replace('$', "\\$"),
            (Ty::List(_), Value::Array(items)) if items.is_empty() => "emptyList()".to_owned(),
            (Ty::Map(_), Value::Object(map)) if map.is_empty() => "emptyMap()".to_owned(),
            (Ty::Ref(name), Value::String(value)) if self.0.is_enum(ty) => {
                format!(
                    "Navree.{name}.{}",
                    screaming(self.0.enum_value(name, value, at)?)
                )
            }
            (Ty::Ref(name), Value::Object(map)) if map.is_empty() => match self.0.defs.get(name) {
                Some(Def::Object(fields)) if fields.iter().all(|field| field.default.is_some()) => {
                    format!("Navree.{name}()")
                }
                _ => {
                    return Err(format!(
                        "{at}: the default {{}} of {name}, which needs fields"
                    ));
                }
            },
            _ => {
                return Err(format!(
                    "{at}: a default the generator can't write: {default}"
                ));
            }
        })
    }

    fn file(&self) -> Result<String, String> {
        let read = self.0;
        let mut out = String::new();
        let _ = writeln!(out, "// {HEADER}");
        out.push_str(
            "\n@file:Suppress(\"unused\", \"RedundantVisibilityModifier\")\n\
             @file:OptIn(kotlinx.serialization.ExperimentalSerializationApi::class)\n\n\
             package com.navree.protocol\n\n\
             import kotlinx.serialization.KSerializer\n\
             import kotlinx.serialization.SerialName\n\
             import kotlinx.serialization.Serializable\n\
             import kotlinx.serialization.builtins.serializer\n\
             import kotlinx.serialization.descriptors.SerialDescriptor\n\
             import kotlinx.serialization.encoding.Decoder\n\
             import kotlinx.serialization.encoding.Encoder\n\
             import kotlinx.serialization.json.Json\n\
             import kotlinx.serialization.json.JsonDecoder\n\
             import kotlinx.serialization.json.JsonElement\n\
             import kotlinx.serialization.json.JsonEncoder\n\
             import kotlinx.serialization.json.JsonObject\n\
             import kotlinx.serialization.json.JsonPrimitive\n\
             import kotlinx.serialization.json.contentOrNull\n\n",
        );
        out.push_str(
            "/**\n * The models of Navree Native's protocol. Read and write them with [Navree.json].\n */\n",
        );
        out.push_str("public object Navree {\n");
        let _ = writeln!(
            out,
            "    /** The protocol's major version these models speak. */\n    public const val MAJOR: Int = {}\n",
            read.major
        );
        let _ = writeln!(
            out,
            "    /** The vocabulary these models read. */\n    public const val VOCAB: Int = {}\n",
            read.vocab
        );
        out.push_str(
            "    /**\n     * How the models are read and written: a field they do not know is passed over,\n     * and a value outside a closed list is read as its field's default.\n     */\n    public val json: Json = Json {\n        ignoreUnknownKeys = true\n        coerceInputValues = true\n    }\n",
        );
        for (name, def) in &read.defs {
            if name.contains('.') {
                continue;
            }
            match def {
                Def::Object(fields) => {
                    let unions = read.unions_of(name);
                    out.push('\n');
                    self.object(&mut out, name, fields, &unions, 1)?;
                }
                Def::Enum(values) => self.enumeration(&mut out, name, values),
                Def::Union {
                    tag,
                    variants,
                    keeps,
                } => self.union(&mut out, name, tag, variants, keeps)?,
                Def::Text => {}
            }
        }
        out.push_str("}\n");
        Ok(out)
    }

    fn object(
        &self,
        out: &mut String,
        name: &str,
        fields: &[Field],
        unions: &[&str],
        depth: usize,
    ) -> Result<(), String> {
        let pad = "    ".repeat(depth);
        let _ = writeln!(out, "{pad}/** `{name}`. */");
        let _ = writeln!(out, "{pad}@Serializable");
        let _ = writeln!(out, "{pad}public data class {}(", last(name));
        for field in fields {
            let property = Self::property(&field.wire);
            if camel(&field.wire) != field.wire {
                let _ = writeln!(out, "{pad}    @SerialName({})", json_string(&field.wire));
            }
            let ty = self.field_ty(field);
            match &field.default {
                Some(default) => {
                    let value = self.default(field, default, &format!("{name}.{}", field.wire))?;
                    let _ = writeln!(out, "{pad}    val {property}: {ty} = {value},");
                }
                None => {
                    let _ = writeln!(out, "{pad}    val {property}: {ty},");
                }
            }
        }
        let parents: Vec<String> = unions
            .iter()
            .map(|union| format!("Navree.{union}"))
            .collect();
        if parents.is_empty() {
            let _ = writeln!(out, "{pad})");
        } else {
            let _ = writeln!(out, "{pad}) : {}", parents.join(", "));
        }
        Ok(())
    }

    fn enumeration(&self, out: &mut String, name: &str, values: &[String]) {
        let _ = writeln!(out, "\n    /** `{name}`. */");
        out.push_str("    @Serializable\n");
        let _ = writeln!(out, "    public enum class {name} {{");
        for value in values {
            let _ = writeln!(
                out,
                "        @SerialName({}) {},",
                json_string(value),
                screaming(value)
            );
        }
        out.push_str("    }\n");
    }

    fn serializer(&self, ty: &Ty, at: &str) -> Result<String, String> {
        Ok(match ty {
            Ty::Text => "String.serializer()".to_owned(),
            Ty::Ref(name) => match self.0.defs.get(name) {
                Some(Def::Union { .. }) => format!("Navree.{name}Serializer"),
                Some(Def::Text) => "String.serializer()".to_owned(),
                _ => format!("Navree.{name}.serializer()"),
            },
            _ => {
                return Err(format!(
                    "{at}: a kept field of a shape the generator can't read"
                ));
            }
        })
    }

    fn union(
        &self,
        out: &mut String,
        name: &str,
        tag: &str,
        variants: &[Variant],
        keeps: &[(String, Ty)],
    ) -> Result<(), String> {
        let read = self.0;
        let _ = writeln!(
            out,
            "\n    /** `{name}`: one of its variants, told apart by `{tag}`. */"
        );
        let _ = writeln!(
            out,
            "    @Serializable(with = Navree.{name}Serializer::class)"
        );
        let _ = writeln!(out, "    public sealed interface {name} {{");
        for variant in variants {
            match &variant.payload {
                Some(payload) if payload.contains('.') => {
                    let Some(Def::Object(fields)) = read.defs.get(payload) else {
                        return Err(format!("{payload}: not an object"));
                    };
                    self.object(out, payload, fields, &[name], 2)?;
                }
                Some(_) => {}
                None => {
                    let _ = writeln!(out, "        /** `{}`. */", variant.tag);
                    let _ = writeln!(
                        out,
                        "        public data object {} : Navree.{name}",
                        camel_type(&variant.tag)
                    );
                }
            }
        }
        let _ = writeln!(
            out,
            "        /** A variant this build does not know, from a newer server, with what it keeps. */"
        );
        let _ = writeln!(out, "        public data class Unknown(");
        out.push_str("            val tag: String,\n");
        for (field, ty) in keeps {
            let _ = writeln!(
                out,
                "            val {}: {}? = null,",
                Self::property(field),
                self.ty(ty)
            );
        }
        let _ = writeln!(out, "        ) : Navree.{name}");
        out.push_str("    }\n");

        // The serializer.
        let tag_text = json_string(tag);
        let _ = writeln!(
            out,
            "\n    /** Reads and writes a [{name}] by its `{tag}`. */"
        );
        let _ = writeln!(
            out,
            "    public object {name}Serializer : KSerializer<Navree.{name}> {{"
        );
        out.push_str(
            "        override val descriptor: SerialDescriptor = JsonElement.serializer().descriptor\n\n",
        );
        let _ = writeln!(
            out,
            "        override fun deserialize(decoder: Decoder): Navree.{name} {{"
        );
        out.push_str("            val input = decoder as JsonDecoder\n");
        let _ = writeln!(
            out,
            "            val fields = input.decodeJsonElement() as? JsonObject ?: return Navree.{name}.Unknown(tag = \"\")"
        );
        let _ = writeln!(
            out,
            "            val tag = (fields[{tag_text}] as? JsonPrimitive)?.contentOrNull"
        );
        let _ = writeln!(
            out,
            "            val body = JsonObject(fields.filterKeys {{ it != {tag_text} }})"
        );
        out.push_str("            return when (tag) {\n");
        for variant in variants {
            let label = json_string(&variant.tag);
            match &variant.payload {
                Some(payload) => {
                    let _ = writeln!(
                        out,
                        "                {label} -> input.json.decodeFromJsonElement(Navree.{payload}.serializer(), body)"
                    );
                }
                None => {
                    let _ = writeln!(
                        out,
                        "                {label} -> Navree.{name}.{}",
                        camel_type(&variant.tag)
                    );
                }
            }
        }
        let _ = writeln!(out, "                else -> Navree.{name}.Unknown(");
        out.push_str("                    tag = tag ?: \"\",\n");
        for (field, ty) in keeps {
            let _ = writeln!(
                out,
                "                    {} = body[{}]?.let {{ input.json.decodeFromJsonElement({}, it) }},",
                Self::property(field),
                json_string(field),
                self.serializer(ty, &format!("{name}.x-unknown.{field}"))?
            );
        }
        out.push_str("                )\n            }\n        }\n\n");
        let _ = writeln!(
            out,
            "        override fun serialize(encoder: Encoder, value: Navree.{name}) {{"
        );
        out.push_str("            val output = encoder as JsonEncoder\n");
        out.push_str("            val tag: String\n            val body: JsonObject\n");
        out.push_str("            when (value) {\n");
        for variant in variants {
            let label = json_string(&variant.tag);
            match &variant.payload {
                Some(payload) => {
                    let _ = writeln!(out, "                is Navree.{payload} -> {{");
                    let _ = writeln!(out, "                    tag = {label}");
                    let _ = writeln!(
                        out,
                        "                    body = output.json.encodeToJsonElement(Navree.{payload}.serializer(), value) as JsonObject"
                    );
                    out.push_str("                }\n");
                }
                None => {
                    let _ = writeln!(
                        out,
                        "                is Navree.{name}.{} -> {{",
                        camel_type(&variant.tag)
                    );
                    let _ = writeln!(out, "                    tag = {label}");
                    out.push_str("                    body = JsonObject(emptyMap())\n");
                    out.push_str("                }\n");
                }
            }
        }
        let _ = writeln!(out, "                is Navree.{name}.Unknown -> {{");
        out.push_str("                    tag = value.tag\n");
        out.push_str("                    val kept = LinkedHashMap<String, JsonElement>()\n");
        for (field, ty) in keeps {
            let property = Self::property(field);
            let _ = writeln!(
                out,
                "                    value.{property}?.let {{ kept[{}] = output.json.encodeToJsonElement({}, it) }}",
                json_string(field),
                self.serializer(ty, &format!("{name}.x-unknown.{field}"))?
            );
        }
        out.push_str("                    body = JsonObject(kept)\n");
        out.push_str("                }\n            }\n");
        let _ = writeln!(
            out,
            "            output.encodeJsonElement(JsonObject(mapOf({tag_text} to JsonPrimitive(tag)) + body))"
        );
        out.push_str("        }\n    }\n");
        Ok(())
    }
}

/// `navigate_external` as `NavigateExternal`: a unit variant's type name.
fn camel_type(tag: &str) -> String {
    let name = camel(tag);
    let mut letters = name.chars();
    letters
        .next()
        .map(|first| first.to_ascii_uppercase().to_string() + letters.as_str())
        .unwrap_or_default()
}

struct Swift<'a>(&'a Read);

const SWIFT_KEYWORDS: &[&str] = &[
    "Any",
    "Self",
    "Type",
    "as",
    "associatedtype",
    "break",
    "case",
    "catch",
    "class",
    "continue",
    "default",
    "defer",
    "deinit",
    "do",
    "else",
    "enum",
    "extension",
    "fallthrough",
    "false",
    "fileprivate",
    "for",
    "func",
    "guard",
    "if",
    "import",
    "in",
    "init",
    "inout",
    "internal",
    "is",
    "let",
    "nil",
    "open",
    "operator",
    "private",
    "protocol",
    "public",
    "repeat",
    "rethrows",
    "return",
    "self",
    "static",
    "struct",
    "subscript",
    "super",
    "switch",
    "throw",
    "throws",
    "true",
    "try",
    "typealias",
    "var",
    "where",
    "while",
];

/// Names the Swift models use without `Navree.` before them, or write into
/// the namespace themselves: a definition of one of these names would hide
/// the type the models mean, inside the namespace, and they would not compile
/// (or would read the wrong type).
const SWIFT_RESERVED: &[&str] = &[
    "Navree",
    "JSONValue",
    "Word",
    "String",
    "Int",
    "Double",
    "Bool",
    "Optional",
    "Array",
    "Dictionary",
    "Codable",
    "Decodable",
    "Encodable",
    "Hashable",
    "Sendable",
    "CaseIterable",
    "RawRepresentable",
    "CodingKey",
    "Decoder",
    "Encoder",
    "Error",
];

impl Swift<'_> {
    /// Refuses a definition whose Swift name is one the models rely on, and a
    /// variant's own type named as the union's `Unknown`.
    fn check_names(&self) -> Result<(), String> {
        for (name, def) in &self.0.defs {
            if matches!(def, Def::Text) {
                // Written as `String`: no type of its own.
                continue;
            }
            let own = last(name);
            if SWIFT_RESERVED.contains(&own) {
                return Err(format!(
                    "{name}: `{own}` would hide the Swift type of that name in the models"
                ));
            }
            if name.contains('.') && own == "Unknown" {
                return Err(format!(
                    "{name}: a union's `Unknown` is the variant this build does not know"
                ));
            }
        }
        Ok(())
    }

    fn name(wire: &str) -> String {
        let name = camel(wire);
        if SWIFT_KEYWORDS.contains(&name.as_str()) {
            format!("`{name}`")
        } else {
            name
        }
    }

    fn type_name(&self, name: &str) -> String {
        match self.0.defs.get(name) {
            Some(Def::Text) => "String".to_owned(),
            _ => format!("Navree.{name}"),
        }
    }

    fn ty(&self, ty: &Ty) -> String {
        match ty {
            Ty::Text => "String".to_owned(),
            Ty::Bool => "Bool".to_owned(),
            Ty::Int | Ty::Long => "Int".to_owned(),
            Ty::Double => "Double".to_owned(),
            Ty::Json => "Navree.JSONValue".to_owned(),
            Ty::List(item) => format!("[{}]", self.ty(item)),
            Ty::Map(value) => format!("[String: {}]", self.ty(value)),
            Ty::Optional(inner) => format!("{}?", self.ty(inner)),
            Ty::Ref(name) => self.type_name(name),
        }
    }

    /// The field's type as declared: optional when its default is `nil`.
    fn field_ty(&self, field: &Field) -> String {
        match (&field.ty, &field.default) {
            (Ty::Json, Some(Value::Null)) => "Navree.JSONValue?".to_owned(),
            (ty, _) => self.ty(ty),
        }
    }

    fn default(&self, field: &Field, default: &Value, at: &str) -> Result<String, String> {
        let ty = match &field.ty {
            Ty::Optional(_) | Ty::Json if default.is_null() => return Ok("nil".to_owned()),
            ty => ty,
        };
        Ok(match (ty, default) {
            (Ty::Bool, Value::Bool(value)) => value.to_string(),
            (Ty::Int | Ty::Long | Ty::Double, Value::Number(value)) => value.to_string(),
            (Ty::Text, Value::String(text)) => json_string(text),
            (Ty::List(_), Value::Array(items)) if items.is_empty() => "[]".to_owned(),
            (Ty::Map(_), Value::Object(map)) if map.is_empty() => "[:]".to_owned(),
            (Ty::Ref(name), Value::String(value)) if self.0.is_enum(ty) => {
                format!(
                    "Navree.{name}.{}",
                    Self::name(self.0.enum_value(name, value, at)?)
                )
            }
            (Ty::Ref(name), Value::Object(map)) if map.is_empty() => match self.0.defs.get(name) {
                Some(Def::Object(fields)) if fields.iter().all(|field| field.default.is_some()) => {
                    format!("Navree.{name}()")
                }
                _ => {
                    return Err(format!(
                        "{at}: the default {{}} of {name}, which needs fields"
                    ));
                }
            },
            _ => {
                return Err(format!(
                    "{at}: a default the generator can't write: {default}"
                ));
            }
        })
    }

    fn file(&self) -> Result<String, String> {
        self.check_names()?;
        let read = self.0;
        let mut out = String::new();
        let _ = writeln!(out, "// {HEADER}\n");
        out.push_str("import Foundation\n\n");
        out.push_str(
            "/// The models of Navree Native's protocol: a field they do not know is passed\n\
             /// over, a variant they do not know is read as `unknown`, and a value outside a\n\
             /// closed list in a field with a default is read as that default.\n",
        );
        out.push_str("public enum Navree {\n");
        let _ = writeln!(
            out,
            "    /// The protocol's major version these models speak.\n    public static let major = {}\n",
            read.major
        );
        let _ = writeln!(
            out,
            "    /// The vocabulary these models read.\n    public static let vocab = {}\n}}",
            read.vocab
        );
        out.push_str(SWIFT_JSON);
        out.push_str(SWIFT_WORD);
        for (name, def) in &read.defs {
            match def {
                Def::Object(fields) => {
                    let (owner, own) = match name.rsplit_once('.') {
                        Some((union, own)) => (format!("Navree.{union}"), own),
                        None => ("Navree".to_owned(), name.as_str()),
                    };
                    let _ = writeln!(out, "\nextension {owner} {{");
                    self.object(&mut out, name, own, fields)?;
                    out.push_str("}\n");
                }
                Def::Enum(values) => {
                    let _ = writeln!(out, "\nextension Navree {{");
                    let _ = writeln!(out, "    /// `{name}`.");
                    let _ = writeln!(
                        out,
                        "    public enum {name}: String, Codable, Hashable, Sendable, CaseIterable {{"
                    );
                    for value in values {
                        let _ = writeln!(
                            out,
                            "        case {} = {}",
                            Self::name(value),
                            json_string(value)
                        );
                    }
                    out.push_str("    }\n}\n");
                }
                Def::Union {
                    tag,
                    variants,
                    keeps,
                } => self.union(&mut out, name, tag, variants, keeps)?,
                Def::Text => {}
            }
        }
        Ok(out)
    }

    fn object(
        &self,
        out: &mut String,
        name: &str,
        own: &str,
        fields: &[Field],
    ) -> Result<(), String> {
        let _ = writeln!(out, "    /// `{name}`.");
        let _ = writeln!(
            out,
            "    public struct {own}: Codable, Hashable, Sendable {{"
        );
        for field in fields {
            let _ = writeln!(
                out,
                "        public var {}: {}",
                Self::name(&field.wire),
                self.field_ty(field)
            );
        }
        // The memberwise initialiser, with each default.
        let mut params = Vec::new();
        for field in fields {
            let name_here = Self::name(&field.wire);
            let ty = self.field_ty(field);
            match &field.default {
                Some(default) => params.push(format!(
                    "{name_here}: {ty} = {}",
                    self.default(field, default, &format!("{name}.{}", field.wire))?
                )),
                None => params.push(format!("{name_here}: {ty}")),
            }
        }
        let _ = writeln!(out, "\n        public init(");
        for (index, param) in params.iter().enumerate() {
            let comma = if index + 1 < params.len() { "," } else { "" };
            let _ = writeln!(out, "            {param}{comma}");
        }
        out.push_str("        ) {\n");
        for field in fields {
            let name_here = Self::name(&field.wire);
            let _ = writeln!(out, "            self.{name_here} = {name_here}");
        }
        out.push_str("        }\n\n");
        out.push_str("        private enum CodingKeys: String, CodingKey {\n");
        for field in fields {
            let _ = writeln!(
                out,
                "            case {} = {}",
                Self::name(&field.wire),
                json_string(&field.wire)
            );
        }
        out.push_str("        }\n\n");
        out.push_str("        public init(from decoder: any Decoder) throws {\n");
        out.push_str(
            "            let container = try decoder.container(keyedBy: CodingKeys.self)\n",
        );
        for field in fields {
            let name_here = Self::name(&field.wire);
            let at = format!("{name}.{}", field.wire);
            let line = match (&field.default, &field.ty) {
                // A list of words of a closed list: the words this build does
                // not know are left out, as the protocol crate reads them.
                (None, Ty::List(inner)) if self.0.is_enum(inner) => format!(
                    "try container.decode([Navree.Word<{}>].self, forKey: .{name_here}).compactMap(\\.value)",
                    self.ty(inner)
                ),
                (Some(default), Ty::List(inner)) if self.0.is_enum(inner) => format!(
                    "try container.decodeIfPresent([Navree.Word<{}>].self, forKey: .{name_here})?.compactMap(\\.value) ?? {}",
                    self.ty(inner),
                    self.default(field, default, &at)?
                ),
                (None, ty) => format!(
                    "try container.decode({}.self, forKey: .{name_here})",
                    self.ty(ty)
                ),
                // An optional value of a closed list: one this build does not
                // know is read as absent.
                (Some(Value::Null), Ty::Optional(inner)) if self.0.is_enum(inner) => format!(
                    "try? container.decodeIfPresent({}.self, forKey: .{name_here})",
                    self.ty(inner)
                ),
                (Some(Value::Null), Ty::Optional(inner)) => format!(
                    "try container.decodeIfPresent({}.self, forKey: .{name_here})",
                    self.ty(inner)
                ),
                (Some(Value::Null), Ty::Json) => format!(
                    "try container.decodeIfPresent(Navree.JSONValue.self, forKey: .{name_here})"
                ),
                // A value of a closed list with a default: one this build does
                // not know is read as the default.
                (Some(default), ty) if self.0.is_enum(ty) => format!(
                    "(try? container.decodeIfPresent({}.self, forKey: .{name_here})) ?? {}",
                    self.ty(ty),
                    self.default(field, default, &at)?
                ),
                (Some(default), ty) => format!(
                    "try container.decodeIfPresent({}.self, forKey: .{name_here}) ?? {}",
                    self.ty(ty),
                    self.default(field, default, &at)?
                ),
            };
            let _ = writeln!(out, "            self.{name_here} = {line}");
        }
        out.push_str("        }\n\n");
        out.push_str("        public func encode(to encoder: any Encoder) throws {\n");
        out.push_str("            var container = encoder.container(keyedBy: CodingKeys.self)\n");
        for field in fields {
            let name_here = Self::name(&field.wire);
            match &field.default {
                None => {
                    let _ = writeln!(
                        out,
                        "            try container.encode(self.{name_here}, forKey: .{name_here})"
                    );
                }
                Some(Value::Null) => {
                    let _ = writeln!(
                        out,
                        "            try container.encodeIfPresent(self.{name_here}, forKey: .{name_here})"
                    );
                }
                Some(default) => {
                    let value = self.default(field, default, &format!("{name}.{}", field.wire))?;
                    let _ = writeln!(
                        out,
                        "            if self.{name_here} != {value} {{ try container.encode(self.{name_here}, forKey: .{name_here}) }}"
                    );
                }
            }
        }
        out.push_str("        }\n    }\n");
        Ok(())
    }

    fn union(
        &self,
        out: &mut String,
        name: &str,
        tag: &str,
        variants: &[Variant],
        keeps: &[(String, Ty)],
    ) -> Result<(), String> {
        let _ = writeln!(out, "\nextension Navree {{");
        let _ = writeln!(
            out,
            "    /// `{name}`: one of its variants, told apart by `{tag}`."
        );
        let _ = writeln!(
            out,
            "    public indirect enum {name}: Codable, Hashable, Sendable {{"
        );
        for variant in variants {
            let case = Self::name(&variant.tag);
            match &variant.payload {
                Some(payload) => {
                    let _ = writeln!(out, "        case {case}(Navree.{payload})");
                }
                None => {
                    let _ = writeln!(out, "        case {case}");
                }
            }
        }
        out.push_str(
            "        /// A variant this build does not know, from a newer server, with what it keeps.\n",
        );
        let _ = writeln!(out, "        case unknown(Navree.{name}.Unknown)\n");
        let _ = writeln!(
            out,
            "        private enum TagKey: String, CodingKey {{\n            case tag = {}\n        }}\n",
            json_string(tag)
        );
        out.push_str("        public init(from decoder: any Decoder) throws {\n");
        out.push_str("            let container = try decoder.container(keyedBy: TagKey.self)\n");
        out.push_str(
            "            let tag = (try? container.decodeIfPresent(String.self, forKey: .tag)) ?? \"\"\n",
        );
        out.push_str("            switch tag {\n");
        for variant in variants {
            let case = Self::name(&variant.tag);
            let label = json_string(&variant.tag);
            match &variant.payload {
                Some(payload) => {
                    let _ = writeln!(
                        out,
                        "            case {label}: self = .{case}(try Navree.{payload}(from: decoder))"
                    );
                }
                None => {
                    let _ = writeln!(out, "            case {label}: self = .{case}");
                }
            }
        }
        let _ = writeln!(
            out,
            "            default: self = .unknown(try Navree.{name}.Unknown(from: decoder, tag: tag))"
        );
        out.push_str("            }\n        }\n\n");
        out.push_str("        public func encode(to encoder: any Encoder) throws {\n");
        out.push_str("            var container = encoder.container(keyedBy: TagKey.self)\n");
        out.push_str("            switch self {\n");
        for variant in variants {
            let case = Self::name(&variant.tag);
            let label = json_string(&variant.tag);
            match &variant.payload {
                Some(_) => {
                    let _ = writeln!(out, "            case .{case}(let value):");
                    let _ = writeln!(
                        out,
                        "                try container.encode({label}, forKey: .tag)"
                    );
                    out.push_str("                try value.encode(to: encoder)\n");
                }
                None => {
                    let _ = writeln!(out, "            case .{case}:");
                    let _ = writeln!(
                        out,
                        "                try container.encode({label}, forKey: .tag)"
                    );
                }
            }
        }
        out.push_str("            case .unknown(let value):\n");
        out.push_str("                try container.encode(value.tag, forKey: .tag)\n");
        out.push_str("                try value.encode(to: encoder)\n");
        out.push_str("            }\n        }\n    }\n}\n");

        // What an unknown variant keeps.
        let _ = writeln!(out, "\nextension Navree.{name} {{");
        let _ = writeln!(
            out,
            "    /// A variant of `{name}` this build does not know: its tag, and what it keeps."
        );
        out.push_str("    public struct Unknown: Hashable, Sendable {\n");
        out.push_str("        public var tag: String\n");
        for (field, ty) in keeps {
            let _ = writeln!(
                out,
                "        public var {}: {}?",
                Self::name(field),
                self.ty(ty)
            );
        }
        out.push_str("\n        public init(tag: String");
        for (field, ty) in keeps {
            let _ = write!(out, ", {}: {}? = nil", Self::name(field), self.ty(ty));
        }
        out.push_str(") {\n            self.tag = tag\n");
        for (field, _) in keeps {
            let field = Self::name(field);
            let _ = writeln!(out, "            self.{field} = {field}");
        }
        out.push_str("        }\n");
        if keeps.is_empty() {
            out.push_str(
                "\n        init(from decoder: any Decoder, tag: String) throws {\n            self.tag = tag\n        }\n\n        func encode(to encoder: any Encoder) throws {}\n",
            );
        } else {
            out.push_str("\n        private enum CodingKeys: String, CodingKey {\n");
            for (field, _) in keeps {
                let _ = writeln!(
                    out,
                    "            case {} = {}",
                    Self::name(field),
                    json_string(field)
                );
            }
            out.push_str("        }\n\n");
            out.push_str("        init(from decoder: any Decoder, tag: String) throws {\n");
            out.push_str(
                "            let container = try decoder.container(keyedBy: CodingKeys.self)\n",
            );
            out.push_str("            self.tag = tag\n");
            for (field, ty) in keeps {
                let field = Self::name(field);
                let _ = writeln!(
                    out,
                    "            self.{field} = try container.decodeIfPresent({}.self, forKey: .{field})",
                    self.ty(ty)
                );
            }
            out.push_str("        }\n\n");
            out.push_str("        func encode(to encoder: any Encoder) throws {\n");
            out.push_str(
                "            var container = encoder.container(keyedBy: CodingKeys.self)\n",
            );
            for (field, _) in keeps {
                let field = Self::name(field);
                let _ = writeln!(
                    out,
                    "            try container.encodeIfPresent(self.{field}, forKey: .{field})"
                );
            }
            out.push_str("        }\n");
        }
        out.push_str("    }\n}\n");
        Ok(())
    }
}

/// Any JSON value, in Swift: what a plugin's view is given.
const SWIFT_JSON: &str = r#"

extension Navree {
    /// Any JSON value.
    public enum JSONValue: Codable, Hashable, Sendable {
        case null
        case bool(Bool)
        case number(Double)
        case string(String)
        case array([Navree.JSONValue])
        case object([String: Navree.JSONValue])

        public init(from decoder: any Decoder) throws {
            let container = try decoder.singleValueContainer()
            if container.decodeNil() {
                self = .null
            } else if let value = try? container.decode(Bool.self) {
                self = .bool(value)
            } else if let value = try? container.decode(Double.self) {
                self = .number(value)
            } else if let value = try? container.decode(String.self) {
                self = .string(value)
            } else if let value = try? container.decode([Navree.JSONValue].self) {
                self = .array(value)
            } else {
                self = .object(try container.decode([String: Navree.JSONValue].self))
            }
        }

        public func encode(to encoder: any Encoder) throws {
            var container = encoder.singleValueContainer()
            switch self {
            case .null: try container.encodeNil()
            case .bool(let value): try container.encode(value)
            case .number(let value): try container.encode(value)
            case .string(let value): try container.encode(value)
            case .array(let value): try container.encode(value)
            case .object(let value): try container.encode(value)
            }
        }
    }
}
"#;

/// One word of a closed list, in Swift: what a list of words is read through.
const SWIFT_WORD: &str = r#"
extension Navree {
    /// One word of a closed list, read leniently: a word this build does not
    /// know is read as none, and a list of words leaves it out. A value of
    /// another shape is still an error.
    struct Word<Value: RawRepresentable>: Decodable where Value.RawValue == String {
        var value: Value?

        init(from decoder: any Decoder) throws {
            let container = try decoder.singleValueContainer()
            self.value = container.decodeNil() ? nil : Value(rawValue: try container.decode(String.self))
        }
    }
}
"#;

struct CSharp<'a>(&'a Read);

impl CSharp<'_> {
    /// `Node.Gap` as `NodeGap`: every definition is a type of its own in the one class.
    fn flat(name: &str) -> String {
        name.replace('.', "")
    }

    /// `view_box` as `ViewBox`.
    fn pascal(wire: &str) -> String {
        let name = camel(wire);
        let mut letters = name.chars();
        letters
            .next()
            .map(|first| first.to_ascii_uppercase().to_string() + letters.as_str())
            .unwrap_or_default()
    }

    /// A field's property, which may not be named as the type that holds it.
    fn property(owner: &str, wire: &str) -> String {
        let name = Self::pascal(wire);
        if name == Self::flat(owner) {
            format!("{name}Field")
        } else {
            name
        }
    }

    fn type_name(&self, name: &str) -> String {
        match self.0.defs.get(name) {
            Some(Def::Text) => "string".to_owned(),
            _ => Self::flat(name),
        }
    }

    fn ty(&self, ty: &Ty) -> String {
        match ty {
            Ty::Text => "string".to_owned(),
            Ty::Bool => "bool".to_owned(),
            Ty::Int => "int".to_owned(),
            Ty::Long => "long".to_owned(),
            Ty::Double => "double".to_owned(),
            Ty::Json => "JsonElement".to_owned(),
            Ty::List(item) => format!("IReadOnlyList<{}>", self.ty(item)),
            Ty::Map(value) => format!("IReadOnlyDictionary<string, {}>", self.ty(value)),
            Ty::Optional(inner) => format!("{}?", self.ty(inner)),
            Ty::Ref(name) => self.type_name(name),
        }
    }

    /// Whether a value of this shape is a struct: an optional one is then a `Nullable`.
    fn is_value(&self, ty: &Ty) -> bool {
        match ty {
            Ty::Bool | Ty::Int | Ty::Long | Ty::Double | Ty::Json => true,
            Ty::Ref(_) => self.0.is_enum(ty),
            _ => false,
        }
    }

    /// The field's type as declared: nullable when its default is `null`.
    fn field_ty(&self, field: &Field) -> String {
        match (&field.ty, &field.default) {
            (Ty::Json, Some(Value::Null)) => "JsonElement?".to_owned(),
            (ty, _) => self.ty(ty),
        }
    }

    fn default(&self, field: &Field, default: &Value, at: &str) -> Result<String, String> {
        let ty = match &field.ty {
            Ty::Optional(_) | Ty::Json if default.is_null() => return Ok("null".to_owned()),
            ty => ty,
        };
        Ok(match (ty, default) {
            (Ty::Bool, Value::Bool(value)) => value.to_string(),
            (Ty::Int, Value::Number(value)) => value.to_string(),
            (Ty::Long, Value::Number(value)) => format!("{value}L"),
            (Ty::Double, Value::Number(value)) => {
                format!("{:?}", value.as_f64().unwrap_or_default())
            }
            (Ty::Text, Value::String(text)) => json_string(text),
            (Ty::List(_), Value::Array(items)) if items.is_empty() => "[]".to_owned(),
            (Ty::Map(value), Value::Object(map)) if map.is_empty() => {
                format!("FrozenDictionary<string, {}>.Empty", self.ty(value))
            }
            (Ty::Ref(name), Value::String(value)) if self.0.is_enum(ty) => {
                format!(
                    "{}.{}",
                    Self::flat(name),
                    Self::pascal(self.0.enum_value(name, value, at)?)
                )
            }
            (Ty::Ref(name), Value::Object(map)) if map.is_empty() => match self.0.defs.get(name) {
                Some(Def::Object(fields)) if fields.iter().all(|field| field.default.is_some()) => {
                    format!("new {}()", Self::flat(name))
                }
                _ => {
                    return Err(format!(
                        "{at}: the default {{}} of {name}, which needs fields"
                    ));
                }
            },
            _ => {
                return Err(format!(
                    "{at}: a default the generator can't write: {default}"
                ));
            }
        })
    }

    /// A `Func<JsonElement, T>` that reads a value of this shape.
    fn reader(&self, ty: &Ty, at: &str) -> Result<String, String> {
        self.reader_at(ty, at, 0)
    }

    fn reader_at(&self, ty: &Ty, at: &str, depth: usize) -> Result<String, String> {
        Ok(match ty {
            Ty::Text => "Reading.Text".to_owned(),
            Ty::Bool => "Reading.Flag".to_owned(),
            Ty::Int => "Reading.Int".to_owned(),
            Ty::Long => "Reading.Long".to_owned(),
            Ty::Double => "Reading.Double".to_owned(),
            Ty::Json => "Reading.Any".to_owned(),
            Ty::Ref(name) => match self.0.defs.get(name) {
                Some(Def::Text) => "Reading.Text".to_owned(),
                _ => format!("Read{}", Self::flat(name)),
            },
            Ty::List(item) => format!(
                "static element{depth} => Reading.ListOf<{}>(element{depth}, {})",
                self.ty(item),
                self.reader_at(item, at, depth + 1)?
            ),
            Ty::Map(item) => format!(
                "static element{depth} => Reading.MapOf<{}>(element{depth}, {})",
                self.ty(item),
                self.reader_at(item, at, depth + 1)?
            ),
            Ty::Optional(inner) => {
                let held = self.ty(inner);
                let read = self.reader_at(inner, at, depth + 1)?;
                let kind = if self.is_value(inner) {
                    "NullV"
                } else {
                    "NullRef"
                };
                format!("static element{depth} => Reading.{kind}<{held}>(element{depth}, {read})")
            }
        })
    }

    /// The expression that reads one field out of `json`.
    fn field_read(&self, owner: &str, field: &Field) -> Result<String, String> {
        let at = format!("{owner}.{}", field.wire);
        let wire = json_string(&field.wire);
        Ok(match (&field.default, &field.ty) {
            (None | Some(Value::Null), Ty::Optional(inner)) => {
                let ty = self.ty(inner);
                let read = self.reader(inner, &at)?;
                if self.0.is_enum(inner) {
                    format!("Reading.OptWord<{ty}>(json, {wire}, {read})")
                } else if self.is_value(inner) {
                    format!("Reading.OptV<{ty}>(json, {wire}, {read})")
                } else {
                    format!("Reading.Opt<{ty}>(json, {wire}, {read})")
                }
            }
            (Some(Value::Null), Ty::Json) => {
                format!("Reading.OptV<JsonElement>(json, {wire}, Reading.Any)")
            }
            (None, ty) => format!(
                "Reading.Required<{}>(json, {wire}, {})",
                self.ty(ty),
                self.reader(ty, &at)?
            ),
            (Some(default), ty) if self.0.is_enum(ty) => format!(
                "Reading.Word<{}>(json, {wire}, {}, {})",
                self.ty(ty),
                self.reader(ty, &at)?,
                self.default(field, default, &at)?
            ),
            (Some(default), ty) => format!(
                "Reading.Or<{}>(json, {wire}, {}, {})",
                self.ty(ty),
                self.reader(ty, &at)?,
                self.default(field, default, &at)?
            ),
        })
    }

    /// The statements that write `value`, of this shape, as a JSON value.
    fn write_value(&self, ty: &Ty, value: &str, depth: usize, at: &str) -> Result<String, String> {
        Ok(match ty {
            Ty::Text => format!("w.WriteStringValue({value});"),
            Ty::Bool => format!("w.WriteBooleanValue({value});"),
            Ty::Int | Ty::Long | Ty::Double => format!("w.WriteNumberValue({value});"),
            Ty::Json => format!("{value}.WriteTo(w);"),
            Ty::Ref(name) => match self.0.defs.get(name) {
                Some(Def::Text) => format!("w.WriteStringValue({value});"),
                _ => format!("Write{}(w, {value});", Self::flat(name)),
            },
            Ty::List(item) => format!(
                "w.WriteStartArray(); foreach (var item{depth} in {value}) {{ {} }} w.WriteEndArray();",
                self.write_value(item, &format!("item{depth}"), depth + 1, at)?
            ),
            Ty::Map(item) => format!(
                "w.WriteStartObject(); foreach (var pair{depth} in {value}) {{ w.WritePropertyName(pair{depth}.Key); {} }} w.WriteEndObject();",
                self.write_value(item, &format!("pair{depth}.Value"), depth + 1, at)?
            ),
            Ty::Optional(inner) => {
                let held = if self.is_value(inner) {
                    format!("{value}.Value")
                } else {
                    value.to_owned()
                };
                format!(
                    "if ({value} is null) {{ w.WriteNullValue(); }} else {{ {} }}",
                    self.write_value(inner, &held, depth + 1, at)?
                )
            }
        })
    }

    /// The statement that writes one field of `v` into the object being written.
    fn field_write(&self, owner: &str, field: &Field) -> Result<String, String> {
        let at = format!("{owner}.{}", field.wire);
        let property = Self::property(owner, &field.wire);
        let name = json_string(&field.wire);
        let access = format!("v.{property}");
        Ok(match (&field.default, &field.ty) {
            (None | Some(Value::Null), Ty::Optional(inner)) => {
                let value = if self.is_value(inner) {
                    format!("{access}.Value")
                } else {
                    access.clone()
                };
                format!(
                    "if ({access} is not null) {{ w.WritePropertyName({name}); {} }}",
                    self.write_value(inner, &value, 1, &at)?
                )
            }
            (Some(Value::Null), Ty::Json) => format!(
                "if ({access} is not null) {{ w.WritePropertyName({name}); {access}.Value.WriteTo(w); }}"
            ),
            (None, ty) => format!(
                "w.WritePropertyName({name}); {}",
                self.write_value(ty, &access, 1, &at)?
            ),
            (Some(default), ty) => {
                let write = self.write_value(ty, &access, 1, &at)?;
                let body = format!("w.WritePropertyName({name}); {write}");
                match ty {
                    Ty::List(_) | Ty::Map(_) => {
                        format!("if ({access}.Count != 0) {{ {body} }}")
                    }
                    Ty::Ref(_) if default.is_object() => body,
                    _ => format!(
                        "if ({access} != {}) {{ {body} }}",
                        self.default(field, default, &at)?
                    ),
                }
            }
        })
    }

    fn file(&self) -> Result<String, String> {
        let read = self.0;
        // Every definition is a type of its own in one class: refuse two with one name.
        let mut seen = BTreeMap::new();
        for (name, def) in &read.defs {
            if matches!(def, Def::Text) {
                continue;
            }
            let mut names = vec![Self::flat(name)];
            if let Def::Union { variants, .. } = def {
                names.push(format!("{}Unknown", Self::flat(name)));
                for variant in variants.iter().filter(|variant| variant.payload.is_none()) {
                    names.push(format!(
                        "{}{}",
                        Self::flat(name),
                        Self::pascal(&variant.tag)
                    ));
                }
            }
            for type_name in names {
                if let Some(other) = seen.insert(type_name.clone(), name.clone()) {
                    return Err(format!(
                        "{name} and {other} are both the C# type {type_name}"
                    ));
                }
            }
        }
        let mut out = String::new();
        let _ = writeln!(out, "// <auto-generated/>\n// {HEADER}\n");
        out.push_str(
            "#nullable enable\n#pragma warning disable CS1591\n\n\
             using System.Collections.Frozen;\n\
             using System.Text.Json;\n\n\
             namespace Navree.Protocol;\n\n",
        );
        out.push_str(
            "/// <summary>\n/// The models of Navree Native's protocol. Read one with <c>ReadX</c> from a\n/// <see cref=\"JsonElement\"/> and write it with <c>WriteX</c> to a\n/// <see cref=\"Utf8JsonWriter\"/>. A field they do not know is passed over, a union's\n/// variant they do not know is read as its <c>Unknown</c>, and a word outside a closed\n/// list is read as its field's default (as <c>Unknown</c> where the field has none).\n/// </summary>\npublic static partial class Models\n{\n",
        );
        let _ = writeln!(
            out,
            "    /// <summary>The protocol's major version these models speak.</summary>\n    public const int Major = {};\n",
            read.major
        );
        let _ = writeln!(
            out,
            "    /// <summary>The vocabulary these models read.</summary>\n    public const int Vocab = {};",
            read.vocab
        );
        for (name, def) in &read.defs {
            match def {
                Def::Object(fields) => {
                    out.push('\n');
                    self.object(&mut out, name, fields, &read.unions_of(name))?;
                }
                Def::Enum(values) => self.enumeration(&mut out, name, values)?,
                Def::Union {
                    tag,
                    variants,
                    keeps,
                } => self.union(&mut out, name, tag, variants, keeps)?,
                Def::Text => {}
            }
        }
        out.push_str("}\n");
        Ok(out)
    }

    fn object(
        &self,
        out: &mut String,
        name: &str,
        fields: &[Field],
        unions: &[&str],
    ) -> Result<(), String> {
        let flat = Self::flat(name);
        let parents: Vec<String> = unions.iter().map(|union| Self::flat(union)).collect();
        let _ = writeln!(out, "    /// <summary><c>{name}</c>.</summary>");
        if parents.is_empty() {
            let _ = writeln!(out, "    public sealed record {flat}");
        } else {
            let _ = writeln!(
                out,
                "    public sealed record {flat} : {}",
                parents.join(", ")
            );
        }
        out.push_str("    {\n");
        for field in fields {
            let property = Self::property(name, &field.wire);
            let ty = self.field_ty(field);
            match &field.default {
                None => {
                    let _ = writeln!(
                        out,
                        "        public required {ty} {property} {{ get; init; }}"
                    );
                }
                Some(Value::Null) => {
                    let _ = writeln!(out, "        public {ty} {property} {{ get; init; }}");
                }
                Some(default) => {
                    let value = self.default(field, default, &format!("{name}.{}", field.wire))?;
                    let _ = writeln!(
                        out,
                        "        public {ty} {property} {{ get; init; }} = {value};"
                    );
                }
            }
        }
        out.push_str("    }\n\n");

        let _ = writeln!(out, "    /// <summary>Reads a <c>{name}</c>.</summary>");
        let _ = writeln!(
            out,
            "    public static {flat} Read{flat}(JsonElement json)\n    {{\n        Reading.ExpectObject(json);\n        return new {flat}\n        {{"
        );
        for field in fields {
            let _ = writeln!(
                out,
                "            {} = {},",
                Self::property(name, &field.wire),
                self.field_read(name, field)?
            );
        }
        out.push_str("        };\n    }\n\n");

        let _ = writeln!(out, "    /// <summary>Writes a <c>{name}</c>.</summary>");
        let _ = writeln!(
            out,
            "    public static void Write{flat}(Utf8JsonWriter w, {flat} v)\n    {{\n        w.WriteStartObject();\n        Write{flat}Body(w, v);\n        w.WriteEndObject();\n    }}\n"
        );
        let _ = writeln!(
            out,
            "    private static void Write{flat}Body(Utf8JsonWriter w, {flat} v)\n    {{"
        );
        for field in fields {
            let _ = writeln!(out, "        {}", self.field_write(name, field)?);
        }
        out.push_str("    }\n");
        Ok(())
    }

    fn enumeration(&self, out: &mut String, name: &str, values: &[String]) -> Result<(), String> {
        let flat = Self::flat(name);
        let mut members = BTreeSet::new();
        for value in values {
            let member = Self::pascal(value);
            if member == "Unknown" || member == flat || !members.insert(member.clone()) {
                return Err(format!("{name}: `{value}` can't be a member of a C# enum"));
            }
            if !member.starts_with(|first: char| first.is_ascii_alphabetic()) {
                return Err(format!("{name}: `{value}` is not a name in C#"));
            }
        }
        let _ = writeln!(out, "\n    /// <summary><c>{name}</c>.</summary>");
        let _ = writeln!(out, "    public enum {flat}\n    {{");
        out.push_str(
            "        /// <summary>A word this build does not know.</summary>\n        Unknown,\n",
        );
        for value in values {
            let _ = writeln!(out, "        {},", Self::pascal(value));
        }
        out.push_str("    }\n\n");
        let _ = writeln!(
            out,
            "    /// <summary>Reads a <c>{name}</c>; a word it does not hold is <c>Unknown</c>.</summary>"
        );
        let _ = writeln!(
            out,
            "    public static {flat} Read{flat}(JsonElement json) =>\n        json.ValueKind == JsonValueKind.String\n            ? json.GetString() switch\n            {{"
        );
        for value in values {
            let _ = writeln!(
                out,
                "                {} => {flat}.{},",
                json_string(value),
                Self::pascal(value)
            );
        }
        let _ = writeln!(
            out,
            "                _ => {flat}.Unknown,\n            }}\n            : {flat}.Unknown;\n"
        );
        let _ = writeln!(
            out,
            "    /// <summary>The word the wire has for a <c>{name}</c>; empty for <c>Unknown</c>.</summary>"
        );
        let _ = writeln!(
            out,
            "    public static string Name{flat}({flat} v) =>\n        v switch\n        {{"
        );
        for value in values {
            let _ = writeln!(
                out,
                "            {flat}.{} => {},",
                Self::pascal(value),
                json_string(value)
            );
        }
        let _ = writeln!(out, "            _ => \"\",\n        }};\n");
        let _ = writeln!(
            out,
            "    /// <summary>Writes a <c>{name}</c>.</summary>\n    public static void Write{flat}(Utf8JsonWriter w, {flat} v) => w.WriteStringValue(Name{flat}(v));"
        );
        Ok(())
    }

    fn union(
        &self,
        out: &mut String,
        name: &str,
        tag: &str,
        variants: &[Variant],
        keeps: &[(String, Ty)],
    ) -> Result<(), String> {
        let flat = Self::flat(name);
        let tag_text = json_string(tag);
        let _ = writeln!(
            out,
            "\n    /// <summary><c>{name}</c>: one of its variants, told apart by <c>{tag}</c>.</summary>"
        );
        let _ = writeln!(out, "    public interface {flat}\n    {{\n    }}\n");
        for variant in variants.iter().filter(|variant| variant.payload.is_none()) {
            let _ = writeln!(
                out,
                "    /// <summary><c>{}</c>.</summary>\n    public sealed record {flat}{} : {flat};\n",
                variant.tag,
                Self::pascal(&variant.tag)
            );
        }
        let _ = writeln!(
            out,
            "    /// <summary>A variant this build does not know, from a newer server, with what it keeps.</summary>"
        );
        let _ = writeln!(
            out,
            "    public sealed record {flat}Unknown : {flat}\n    {{"
        );
        out.push_str("        public required string Tag { get; init; }\n");
        for (field, ty) in keeps {
            let _ = writeln!(
                out,
                "        public {}? {} {{ get; init; }}",
                self.ty(ty),
                Self::property(&format!("{name}.Unknown"), field)
            );
        }
        out.push_str("    }\n\n");

        // Reading.
        let _ = writeln!(
            out,
            "    /// <summary>Reads a <c>{name}</c> by its <c>{tag}</c>.</summary>"
        );
        let _ = writeln!(
            out,
            "    public static {flat} Read{flat}(JsonElement json)\n    {{\n        if (json.ValueKind != JsonValueKind.Object)\n        {{\n            return new {flat}Unknown {{ Tag = \"\" }};\n        }}\n\n        string tag = Reading.Tag(json, {tag_text});\n        switch (tag)\n        {{"
        );
        for variant in variants {
            let label = json_string(&variant.tag);
            match &variant.payload {
                Some(payload) => {
                    let _ = writeln!(
                        out,
                        "            case {label}: return Read{}(json);",
                        Self::flat(payload)
                    );
                }
                None => {
                    let _ = writeln!(
                        out,
                        "            case {label}: return new {flat}{}();",
                        Self::pascal(&variant.tag)
                    );
                }
            }
        }
        out.push_str("            default:\n");
        let _ = writeln!(
            out,
            "                return new {flat}Unknown\n                {{\n                    Tag = tag,"
        );
        for (field, ty) in keeps {
            let at = format!("{name}.x-unknown.{field}");
            let read = self.reader(ty, &at)?;
            let kind = if self.is_value(ty) { "OptV" } else { "Opt" };
            let _ = writeln!(
                out,
                "                    {} = Reading.{kind}<{}>(json, {}, {read}),",
                Self::property(&format!("{name}.Unknown"), field),
                self.ty(ty),
                json_string(field)
            );
        }
        out.push_str("                };\n        }\n    }\n\n");

        // Writing.
        let _ = writeln!(
            out,
            "    /// <summary>Writes a <c>{name}</c>, its <c>{tag}</c> first.</summary>"
        );
        let _ = writeln!(
            out,
            "    public static void Write{flat}(Utf8JsonWriter w, {flat} v)\n    {{\n        w.WriteStartObject();\n        switch (v)\n        {{"
        );
        for variant in variants {
            let label = json_string(&variant.tag);
            match &variant.payload {
                Some(payload) => {
                    let _ = writeln!(
                        out,
                        "            case {} x:\n                w.WriteString({tag_text}, {label});\n                Write{}Body(w, x);\n                break;",
                        Self::flat(payload),
                        Self::flat(payload)
                    );
                }
                None => {
                    let _ = writeln!(
                        out,
                        "            case {flat}{}:\n                w.WriteString({tag_text}, {label});\n                break;",
                        Self::pascal(&variant.tag)
                    );
                }
            }
        }
        let _ = writeln!(
            out,
            "            case {flat}Unknown x:\n                w.WriteString({tag_text}, x.Tag);"
        );
        for (field, ty) in keeps {
            let at = format!("{name}.x-unknown.{field}");
            let property = Self::property(&format!("{name}.Unknown"), field);
            let value = if self.is_value(ty) {
                format!("x.{property}.Value")
            } else {
                format!("x.{property}")
            };
            let _ = writeln!(
                out,
                "                if (x.{property} is not null) {{ w.WritePropertyName({}); {} }}",
                json_string(field),
                self.write_value(ty, &value, 1, &at)?
            );
        }
        let _ = writeln!(
            out,
            "                break;\n            default:\n                throw new ArgumentException(\"a {name} this build does not know\", nameof(v));\n        }}\n        w.WriteEndObject();\n    }}"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A schema with one of each thing the generator writes.
    fn small() -> Value {
        json!({
            "x-major": 1,
            "x-vocab": 0,
            "$defs": {
                "Tone": {"x-kind": "enum", "type": "string", "enum": ["neutral", "large_title"]},
                "Path": {"x-kind": "string", "type": "string", "format": "origin-path"},
                "Badge": {
                    "x-kind": "object",
                    "properties": {
                        "text": {"type": "string"},
                        "view_box": {"anyOf": [{"type": "string"}, {"type": "null"}], "default": null},
                        "tone": {"$ref": "#/$defs/Tone", "default": "neutral"},
                        "in": {"type": "boolean", "default": true},
                        "count": {"type": "integer", "format": "uint32", "default": 0},
                        "href": {"$ref": "#/$defs/Path"},
                        "props": {"x-json": true, "default": null},
                        "items": {"type": "array", "items": {"$ref": "#/$defs/Node"}, "default": []},
                        "tones": {"type": "array", "items": {"$ref": "#/$defs/Tone"}, "default": []}
                    },
                    "required": ["text", "href"]
                },
                "Node.Gap": {
                    "x-kind": "object",
                    "properties": {"size": {"type": "integer", "format": "uint8"}},
                    "required": ["size"]
                },
                "Node": {
                    "x-kind": "union",
                    "x-tag": "t",
                    "x-variants": [
                        {"tag": "badge", "type": "Badge"},
                        {"tag": "gap", "type": "Node.Gap"},
                        {"tag": "line_break"}
                    ],
                    "x-unknown": {"alt": {"$ref": "#/$defs/Node"}}
                }
            }
        })
    }

    #[test]
    fn kotlin_writes_classes_enums_and_a_serializer_per_union() {
        let models = generate(&small()).unwrap();
        let kotlin = &models.kotlin;
        for line in [
            "public const val MAJOR: Int = 1",
            "public data class Badge(",
            "    val text: String,",
            "    @SerialName(\"view_box\")\n        val viewBox: String? = null,",
            "    val tone: Navree.Tone = Navree.Tone.NEUTRAL,",
            "    val `in`: Boolean = true,",
            "    val count: Long = 0L,",
            "    val href: String,",
            "    val props: JsonElement? = null,",
            "    val items: kotlin.collections.List<Navree.Node> = emptyList(),",
            ") : Navree.Node",
            "@SerialName(\"large_title\") LARGE_TITLE,",
            "public sealed interface Node {",
            "public data class Gap(",
            "public data object LineBreak : Navree.Node",
            "\"badge\" -> input.json.decodeFromJsonElement(Navree.Badge.serializer(), body)",
            "\"gap\" -> input.json.decodeFromJsonElement(Navree.Node.Gap.serializer(), body)",
            "\"line_break\" -> Navree.Node.LineBreak",
            "alt = body[\"alt\"]?.let { input.json.decodeFromJsonElement(Navree.NodeSerializer, it) },",
            "val alt: Navree.Node? = null,",
        ] {
            assert!(kotlin.contains(line), "missing {line:?} in\n{kotlin}");
        }
    }

    #[test]
    fn swift_writes_structs_with_their_own_coding_and_an_enum_per_union() {
        let models = generate(&small()).unwrap();
        let swift = &models.swift;
        for line in [
            "public static let major = 1",
            "public struct Badge: Codable, Hashable, Sendable {",
            "public var `in`: Bool",
            "case viewBox = \"view_box\"",
            "self.text = try container.decode(String.self, forKey: .text)",
            "self.tone = (try? container.decodeIfPresent(Navree.Tone.self, forKey: .tone)) ?? Navree.Tone.neutral",
            "self.count = try container.decodeIfPresent(Int.self, forKey: .count) ?? 0",
            "self.props = try container.decodeIfPresent(Navree.JSONValue.self, forKey: .props)",
            "if self.items != [] { try container.encode(self.items, forKey: .items) }",
            "try container.encodeIfPresent(self.viewBox, forKey: .viewBox)",
            "self.tones = try container.decodeIfPresent([Navree.Word<Navree.Tone>].self, forKey: .tones)?.compactMap(\\.value) ?? []",
            "struct Word<Value: RawRepresentable>: Decodable where Value.RawValue == String {",
            "case largeTitle = \"large_title\"",
            "public indirect enum Node: Codable, Hashable, Sendable {",
            "case badge(Navree.Badge)",
            "case gap(Navree.Node.Gap)",
            "case lineBreak",
            "case \"gap\": self = .gap(try Navree.Node.Gap(from: decoder))",
            "default: self = .unknown(try Navree.Node.Unknown(from: decoder, tag: tag))",
            "extension Navree.Node {\n    /// `Node.Gap`.",
            "public var alt: Navree.Node?",
        ] {
            assert!(swift.contains(line), "missing {line:?} in\n{swift}");
        }
    }

    #[test]
    fn csharp_writes_records_enums_an_interface_per_union_and_a_reader_and_writer_each() {
        let models = generate(&small()).unwrap();
        let csharp = &models.csharp;
        for line in [
            "public const int Major = 1;",
            "public sealed record Badge : Node",
            "public required string Text { get; init; }",
            "public string? ViewBox { get; init; }",
            "public Tone Tone { get; init; } = Tone.Neutral;",
            "public bool In { get; init; } = true;",
            "public long Count { get; init; } = 0L;",
            "public required string Href { get; init; }",
            "public JsonElement? Props { get; init; }",
            "public IReadOnlyList<Node> Items { get; init; } = [];",
            "public enum Tone",
            "Unknown,",
            "LargeTitle,",
            "\"large_title\" => Tone.LargeTitle,",
            "public interface Node",
            "public sealed record NodeGap : Node",
            "public sealed record NodeLineBreak : Node;",
            "public sealed record NodeUnknown : Node",
            "public Node? Alt { get; init; }",
            "public static Badge ReadBadge(JsonElement json)",
            "ViewBox = Reading.Opt<string>(json, \"view_box\", Reading.Text),",
            "Tone = Reading.Word<Tone>(json, \"tone\", ReadTone, Tone.Neutral),",
            "Count = Reading.Or<long>(json, \"count\", Reading.Long, 0L),",
            "Props = Reading.OptV<JsonElement>(json, \"props\", Reading.Any),",
            "Items = Reading.Or<IReadOnlyList<Node>>(json, \"items\", static element0 => Reading.ListOf<Node>(element0, ReadNode), []),",
            "case \"gap\": return ReadNodeGap(json);",
            "case \"line_break\": return new NodeLineBreak();",
            "Alt = Reading.Opt<Node>(json, \"alt\", ReadNode),",
            "if (v.Tone != Tone.Neutral) { w.WritePropertyName(\"tone\"); WriteTone(w, v.Tone); }",
            "if (v.Items.Count != 0) {",
            "if (v.ViewBox is not null) {",
            "case Badge x:",
            "w.WriteString(\"t\", \"badge\");",
        ] {
            assert!(csharp.contains(line), "missing {line:?} in\n{csharp}");
        }
    }

    #[test]
    fn csharp_gives_a_field_named_as_its_type_another_property_name() {
        let mut schema = small();
        schema["$defs"]["Badge"]["properties"]["badge"] = json!({"type": "string", "default": ""});
        let csharp = generate(&schema).unwrap().csharp;
        assert!(csharp.contains("public string BadgeField { get; init; } = \"\";"));
    }

    #[test]
    fn what_a_language_could_not_hold_is_refused() {
        let mut schema = small();
        schema["$defs"]["Badge"]["properties"]["tone"]["default"] = json!("loud");
        assert_eq!(
            generate(&schema).unwrap_err(),
            "Badge.tone: the default `loud` is not a Tone"
        );
        let mut schema = small();
        schema["$defs"]["Badge"]["properties"]["again"] =
            json!({"anyOf": [{"$ref": "#/$defs/Badge"}, {"type": "null"}], "default": null});
        assert_eq!(
            generate(&schema).unwrap_err(),
            "Badge holds itself with no list or union between"
        );
        let mut schema = small();
        schema["$defs"]["Badge"]["properties"]["count"]["default"] = json!([1]);
        assert!(generate(&schema).unwrap_err().contains("Badge.count"));
        let mut schema = small();
        schema["$defs"]["Badge"]["properties"]["missing"] = json!({"$ref": "#/$defs/Nowhere"});
        schema["$defs"]["Badge"]["required"] = json!(["text", "href", "missing"]);
        assert_eq!(
            generate(&schema).unwrap_err(),
            "Badge.missing: refers to `Nowhere`, which is not defined"
        );
    }

    #[test]
    fn a_swift_name_the_models_rely_on_is_refused() {
        for name in ["String", "Decoder", "Navree", "Word"] {
            let mut schema = small();
            schema["$defs"][name] = json!({"x-kind": "enum", "type": "string", "enum": ["a"]});
            assert_eq!(
                generate(&schema).unwrap_err(),
                format!("{name}: `{name}` would hide the Swift type of that name in the models")
            );
        }
        let mut schema = small();
        schema["$defs"]["Node.Unknown"] = schema["$defs"]["Node.Gap"].clone();
        schema["$defs"]["Node"]["x-variants"]
            .as_array_mut()
            .unwrap()
            .push(json!({"tag": "unknown", "type": "Node.Unknown"}));
        assert!(generate(&schema).unwrap_err().starts_with("Node.Unknown: "));
    }

    #[test]
    fn the_check_names_each_stale_file_and_writing_makes_them_current() {
        let root = tempfile::tempdir().unwrap();
        let schema = root.path().join(SCHEMA);
        std::fs::create_dir_all(schema.parent().unwrap()).unwrap();
        std::fs::write(&schema, serde_json::to_string(&small()).unwrap()).unwrap();
        let stale = run(root.path(), true).unwrap();
        assert_eq!(
            stale,
            [
                root.path().join(KOTLIN),
                root.path().join(SWIFT),
                root.path().join(CSHARP)
            ]
        );
        assert!(run(root.path(), false).unwrap().is_empty());
        assert!(run(root.path(), true).unwrap().is_empty());
        std::fs::write(root.path().join(SWIFT), "// changed by hand\n").unwrap();
        assert_eq!(run(root.path(), true).unwrap(), [root.path().join(SWIFT)]);
    }

    #[test]
    fn the_protocol_schema_is_written_without_error() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(SCHEMA);
        let schema: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let models = generate(&schema).unwrap();
        assert!(models.kotlin.contains("public sealed interface Node {"));
        assert!(
            models
                .swift
                .contains("public indirect enum Node: Codable, Hashable, Sendable {")
        );
        assert!(models.csharp.contains("public interface Node"));
        assert!(
            models
                .csharp
                .contains("public static Node ReadNode(JsonElement json)")
        );
    }
}
