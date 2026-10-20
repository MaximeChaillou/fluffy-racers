//! The tuning resources seen by reflection (issue #36): every field of `KartTuning`,
//! `CroquetteTuning`... is walked without being named, so that a field added later shows up in
//! the admin panel, its export and its storage with no extra code.
//!
//! Pure logic, no egui: the leaves of a value ([`fields`]), writing one by its path ([`set`]),
//! and the registry of the game's tunings ([`Tunings`]): what differs from the defaults, the
//! export of only those values as RON text, and the import of such a text.
//!
//! Since issue #155, the registry also holds the « Ressenti » knobs ([`Feel`], `super::feel`):
//! each one drives several values. A value's **baseline** is what the knobs give it (its default
//! when its knob is in the middle, or when no knob drives it); a value off its baseline was
//! **modified by hand**, and its knob leaves it alone until it is back to its baseline.
//!
//! The text is a RON map from `"Resource.path"` to the value, with the baseline in a comment; the
//! knobs off the middle come first, as `"Ressenti.<knob>"`; only the values modified by hand
//! follow:
//!
//! ```text
//! // Fluffy Racers : réglages du panneau d'admin (#36, #155), le reste est par défaut.
//! {
//!     "Ressenti.derapage": 70, // de 0 à 100, milieu : 50
//!     "KartTuning.max_speed": 30.0, // défaut : 28.5
//!     "KartTuning.drift_radius_tight": 21.0, // curseur Dérapage : 19.95, défaut : 17.5
//!     "BotTuning.skills[1].target_tier": "Purple", // défaut : "Orange"
//! }
//! ```

use std::collections::BTreeMap;
use std::fmt::{self, Write as _};

use bevy::ecs::component::Mutable;
use bevy::prelude::*;
use bevy::reflect::enums::{DynamicEnum, DynamicVariant, VariantInfo, VariantType};
use bevy::reflect::{GetPath, PartialReflect, ReflectRef, TypeInfo, TypePath};
use serde::Deserialize;
use serde::de::{self, Visitor};

use super::feel::{self, FEEL_PREFIX, Feel, KNOB_MAX, Knob, LINKS, Link, MIDDLE};

/// A leaf of a tuning: a number, a switch or the variant of a simple enum.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    F32(f32),
    F64(f64),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    Usize(usize),
    I32(i32),
    I64(i64),
    Bool(bool),
    /// A variant without data of an enum (`BotDifficulty`, `MiniTurbo`...).
    Variant(&'static str),
}

impl Value {
    /// As a number, for sliders (`None` for switches and variants).
    pub fn as_f64(&self) -> Option<f64> {
        Some(match *self {
            Value::F32(v) => v as f64,
            Value::F64(v) => v,
            Value::U8(v) => v as f64,
            Value::U16(v) => v as f64,
            Value::U32(v) => v as f64,
            Value::U64(v) => v as f64,
            Value::Usize(v) => v as f64,
            Value::I32(v) => v as f64,
            Value::I64(v) => v as f64,
            Value::Bool(_) | Value::Variant(_) => return None,
        })
    }

    /// Whether it is a whole number (sliders step by one).
    pub fn is_integer(&self) -> bool {
        !matches!(self, Value::F32(_) | Value::F64(_)) && self.as_f64().is_some()
    }

    /// The same kind of value holding `number` (rounded and clamped for integers). `None` for
    /// switches and variants.
    pub fn with_number(&self, number: f64) -> Option<Value> {
        let whole = number.round();
        Some(match self {
            Value::F32(_) => Value::F32(number as f32),
            Value::F64(_) => Value::F64(number),
            Value::U8(_) => Value::U8(whole.clamp(0.0, u8::MAX as f64) as u8),
            Value::U16(_) => Value::U16(whole.clamp(0.0, u16::MAX as f64) as u16),
            Value::U32(_) => Value::U32(whole.clamp(0.0, u32::MAX as f64) as u32),
            Value::U64(_) => Value::U64(whole.max(0.0) as u64),
            Value::Usize(_) => Value::Usize(whole.max(0.0) as usize),
            Value::I32(_) => Value::I32(whole.clamp(i32::MIN as f64, i32::MAX as f64) as i32),
            Value::I64(_) => Value::I64(whole as i64),
            Value::Bool(_) | Value::Variant(_) => return None,
        })
    }
}

impl fmt::Display for Value {
    /// RON: floats always with a decimal point (`30.0`), variants as strings (`"Hard"`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // `Debug` is the shortest text that reads back to the same float, with ".0".
            Value::F32(v) => write!(f, "{v:?}"),
            Value::F64(v) => write!(f, "{v:?}"),
            Value::U8(v) => write!(f, "{v}"),
            Value::U16(v) => write!(f, "{v}"),
            Value::U32(v) => write!(f, "{v}"),
            Value::U64(v) => write!(f, "{v}"),
            Value::Usize(v) => write!(f, "{v}"),
            Value::I32(v) => write!(f, "{v}"),
            Value::I64(v) => write!(f, "{v}"),
            Value::Bool(v) => write!(f, "{v}"),
            Value::Variant(name) => write!(f, "{name:?}"),
        }
    }
}

/// A leaf of a tuning, by its reflection path inside it (`max_speed`, `inertia.y`,
/// `skills[1].wobble`).
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub path: String,
    pub value: Value,
    /// The variants to choose from, for an enum.
    pub variants: &'static [&'static str],
}

/// Every leaf of `value`, in declaration order: struct fields, tuple struct fields, array and
/// list items, nested as deep as they go. Values of other kinds (maps, strings...) are skipped.
pub fn fields(value: &dyn PartialReflect) -> Vec<Field> {
    let mut out = Vec::new();
    walk(value, &mut String::new(), &mut |path, value, variants| {
        out.push(Field {
            path: path.to_string(),
            value,
            variants,
        })
    });
    out
}

/// Only the values of [`fields`], in the same order, into `out` (no allocation once its
/// capacity is reached).
pub fn values_into(value: &dyn PartialReflect, path: &mut String, out: &mut Vec<Value>) {
    out.clear();
    path.clear();
    walk(value, path, &mut |_, value, _| out.push(value));
}

fn walk(
    value: &dyn PartialReflect,
    path: &mut String,
    visit: &mut dyn FnMut(&str, Value, &'static [&'static str]),
) {
    let start = path.len();
    match value.reflect_ref() {
        ReflectRef::Struct(s) => {
            for i in 0..s.field_len() {
                let (Some(name), Some(field)) = (s.name_at(i), s.field_at(i)) else {
                    continue;
                };
                if !path.is_empty() {
                    path.push('.');
                }
                path.push_str(name);
                walk(field, path, visit);
                path.truncate(start);
            }
        }
        ReflectRef::TupleStruct(s) => {
            for i in 0..s.field_len() {
                if let Some(field) = s.field(i) {
                    let _ = write!(path, ".{i}");
                    walk(field, path, visit);
                    path.truncate(start);
                }
            }
        }
        ReflectRef::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                let _ = write!(path, "[{i}]");
                walk(item, path, visit);
                path.truncate(start);
            }
        }
        ReflectRef::List(items) => {
            for (i, item) in items.iter().enumerate() {
                let _ = write!(path, "[{i}]");
                walk(item, path, visit);
                path.truncate(start);
            }
        }
        ReflectRef::Enum(e) => {
            if e.variant_type() == VariantType::Unit
                && let Some((name, variants)) = static_variant(value, e.variant_name())
            {
                visit(path, Value::Variant(name), variants);
            }
        }
        _ => {
            if let Some(leaf) = leaf(value) {
                visit(path, leaf, &[]);
            }
        }
    }
}

/// The variant names of the enum `value`, when they all are unit variants, and `name` among
/// them.
fn static_variant(
    value: &dyn PartialReflect,
    name: &str,
) -> Option<(&'static str, &'static [&'static str])> {
    let Some(TypeInfo::Enum(info)) = value.get_represented_type_info() else {
        return None;
    };
    let all_unit = info
        .iter()
        .all(|variant| matches!(variant, VariantInfo::Unit(_)));
    if !all_unit {
        return None;
    }
    let variants = info.variant_names();
    variants
        .iter()
        .find(|v| **v == name)
        .map(|found| (*found, variants))
}

fn leaf(value: &dyn PartialReflect) -> Option<Value> {
    macro_rules! read {
        ($($ty:ty => $variant:ident),*) => {
            $(if let Some(v) = value.try_downcast_ref::<$ty>() {
                return Some(Value::$variant(*v));
            })*
        };
    }
    read!(f32 => F32, f64 => F64, u8 => U8, u16 => U16, u32 => U32, u64 => U64,
          usize => Usize, i32 => I32, i64 => I64, bool => Bool);
    None
}

/// Why a value could not be written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetError {
    /// No such field (renamed or removed since the text was written).
    UnknownField(String),
    /// The field holds another kind of value.
    WrongKind(String),
    /// Not a variant of the field's enum.
    UnknownVariant(String),
}

impl fmt::Display for SetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SetError::UnknownField(path) => write!(f, "{path} : réglage inconnu"),
            SetError::WrongKind(path) => write!(f, "{path} : type de valeur incorrect"),
            SetError::UnknownVariant(path) => write!(f, "{path} : choix inconnu"),
        }
    }
}

/// Writes `value` at `path` inside `root` (a path from [`fields`]). The field must hold the same
/// kind of value.
pub fn set(root: &mut dyn Reflect, path: &str, value: Value) -> Result<(), SetError> {
    let target = root
        .reflect_path_mut(path)
        .map_err(|_| SetError::UnknownField(path.to_string()))?;
    macro_rules! assign {
        ($($variant:ident => $ty:ty),*) => {
            match value {
                $(Value::$variant(v) => {
                    *target
                        .try_downcast_mut::<$ty>()
                        .ok_or_else(|| SetError::WrongKind(path.to_string()))? = v;
                })*
                Value::Variant(name) => {
                    let known = matches!(
                        target.get_represented_type_info(),
                        Some(TypeInfo::Enum(info)) if info.contains_variant(name)
                    ) && static_variant(target, name).is_some();
                    if !known {
                        return Err(SetError::UnknownVariant(path.to_string()));
                    }
                    target
                        .try_apply(&DynamicEnum::new(name, DynamicVariant::Unit))
                        .map_err(|_| SetError::WrongKind(path.to_string()))?;
                }
            }
        };
    }
    assign!(F32 => f32, F64 => f64, U8 => u8, U16 => u16, U32 => u32, U64 => u64,
            Usize => usize, I32 => i32, I64 => i64, Bool => bool);
    Ok(())
}

/// A value as read from a pasted text, before it meets the field it goes to.
#[derive(Clone, Debug, PartialEq)]
pub enum Raw {
    Int(i128),
    Float(f64),
    Bool(bool),
    Text(String),
}

impl Raw {
    /// The value for a field currently holding `like`, if it fits (integers in range, a known
    /// variant name).
    pub fn to_value(&self, like: &Field) -> Option<Value> {
        match (self, like.value) {
            (Raw::Bool(b), Value::Bool(_)) => Some(Value::Bool(*b)),
            (Raw::Text(name), Value::Variant(_)) => like
                .variants
                .iter()
                .copied()
                .find(|v| *v == name.as_str())
                .map(Value::Variant),
            (Raw::Float(x), Value::F32(_) | Value::F64(_)) => like.value.with_number(*x),
            (Raw::Int(i), Value::F32(_) | Value::F64(_)) => like.value.with_number(*i as f64),
            (Raw::Int(i), current) if current.is_integer() => {
                let fits = match current {
                    Value::U8(_) => u8::try_from(*i).is_ok(),
                    Value::U16(_) => u16::try_from(*i).is_ok(),
                    Value::U32(_) => u32::try_from(*i).is_ok(),
                    Value::U64(_) => u64::try_from(*i).is_ok(),
                    Value::Usize(_) => usize::try_from(*i).is_ok(),
                    Value::I32(_) => i32::try_from(*i).is_ok(),
                    Value::I64(_) => i64::try_from(*i).is_ok(),
                    _ => false,
                };
                fits.then(|| current.with_number(*i as f64)).flatten()
            }
            _ => None,
        }
    }
}

impl<'de> Deserialize<'de> for Raw {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RawVisitor;
        impl Visitor<'_> for RawVisitor {
            type Value = Raw;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a number, true / false or a quoted name")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Raw, E> {
                Ok(Raw::Bool(v))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Raw, E> {
                Ok(Raw::Int(v as i128))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Raw, E> {
                Ok(Raw::Int(v as i128))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Raw, E> {
                Ok(Raw::Float(v))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Raw, E> {
                Ok(Raw::Text(v.to_string()))
            }
        }
        deserializer.deserialize_any(RawVisitor)
    }
}

/// Parses a text of [`Tunings::export`]: `"Resource.path": value` pairs. Comments are fine.
pub fn parse(text: &str) -> Result<BTreeMap<String, Raw>, String> {
    if text.trim().is_empty() {
        return Ok(BTreeMap::new());
    }
    ron::from_str(text).map_err(|error| error.to_string())
}

/// A value modified by hand (off its baseline): `tuning` (the resource's name) and the field's
/// `path` in it.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub tuning: &'static str,
    pub path: String,
    pub value: Value,
    pub default: Value,
    /// The knob that drives it and the value it gives, when that knob is off the middle.
    pub driven: Option<(Knob, Value)>,
}

/// What an import did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportReport {
    /// Values written.
    pub applied: usize,
    /// Entries left out, and why (unknown tuning or field, value that does not fit).
    pub ignored: Vec<String>,
}

/// One tuning resource, seen by reflection.
pub struct Tuning {
    /// The resource's type name (`KartTuning`): the first part of the paths of the export.
    pub name: &'static str,
    read: fn(&World) -> Option<&dyn Reflect>,
    write: fn(&mut World) -> Option<Mut<'_, dyn Reflect>>,
    reset: fn(&mut World),
    /// The default value's leaves, computed once.
    pub defaults: Vec<Field>,
    /// The « Ressenti » link of each of them (an index in `feel::LINKS`), in the same order.
    links: Vec<Option<usize>>,
}

impl Tuning {
    fn of<T: Resource + Component<Mutability = Mutable> + Reflect + Default + TypePath>() -> Self {
        let name = T::short_type_path();
        let defaults = fields(&T::default());
        let links = defaults
            .iter()
            .map(|f| feel::link_for(&format!("{name}.{}", f.path)))
            .collect();
        Self {
            name,
            read: |world| world.get_resource::<T>().map(|r| r as &dyn Reflect),
            write: |world| {
                world
                    .get_resource_mut::<T>()
                    .map(|r| r.map_unchanged(|r| r as &mut dyn Reflect))
            },
            reset: |world| {
                if let Some(mut r) = world.get_resource_mut::<T>() {
                    *r = T::default();
                }
            },
            defaults,
            links,
        }
    }

    /// The resource, if the game has it.
    pub fn read<'w>(&self, world: &'w World) -> Option<&'w dyn Reflect> {
        (self.read)(world)
    }

    /// Writes one field, marking the resource changed only if the value differs.
    pub fn set(&self, world: &mut World, path: &str, value: Value) -> Result<(), SetError> {
        let current = self.read(world).and_then(|r| r.reflect_path(path).ok());
        let Some(current) = current else {
            return Err(SetError::UnknownField(format!("{}.{path}", self.name)));
        };
        if leaf_or_variant(current) == Some(value) {
            return Ok(());
        }
        let Some(mut resource) = (self.write)(world) else {
            return Err(SetError::UnknownField(format!("{}.{path}", self.name)));
        };
        set(&mut *resource, path, value)
    }

    /// Back to the default value (the whole resource).
    pub fn reset(&self, world: &mut World) {
        (self.reset)(world);
    }

    /// The default of the field at `path`.
    pub fn default_of(&self, path: &str) -> Option<&Field> {
        self.defaults.iter().find(|f| f.path == path)
    }

    /// The default and the « Ressenti » link of the field at `path`, `index` being where it
    /// usually is (its place among the fields: no search then).
    fn default_and_link(
        &self,
        index: usize,
        path: &str,
    ) -> Option<(&Field, Option<&'static Link>)> {
        let index = match self.defaults.get(index) {
            Some(field) if field.path == path => index,
            _ => self.defaults.iter().position(|f| f.path == path)?,
        };
        let link = self.links[index].map(|i| &LINKS[i]);
        Some((&self.defaults[index], link))
    }

    /// The knob that drives the field at `path`, if any.
    pub fn knob_of(&self, path: &str) -> Option<Knob> {
        let index = self.defaults.iter().position(|f| f.path == path)?;
        self.links[index].map(|i| LINKS[i].knob)
    }

    /// What the knobs of `feel` give the field at `path` (`index`: its usual place): its default
    /// when no knob drives it or its knob is in the middle. With the knob, when it is off the
    /// middle.
    fn baseline_at(
        &self,
        feel: &Feel,
        index: usize,
        path: &str,
    ) -> Option<(Value, Value, Option<(Knob, Value)>)> {
        let (default, link) = self.default_and_link(index, path)?;
        let driven = link
            .map(|l| (l.knob, feel.position(l.knob)))
            .filter(|(_, position)| *position != MIDDLE)
            .map(|(knob, position)| (knob, link_value(link, default.value, position)));
        let baseline = driven.map_or(default.value, |(_, value)| value);
        Some((baseline, default.value, driven))
    }

    /// The baseline of the field at `path` (see [`Tuning::baseline_at`]).
    pub fn baseline(&self, feel: &Feel, path: &str) -> Option<Value> {
        self.baseline_at(feel, usize::MAX, path).map(|(b, _, _)| b)
    }
}

/// The value `link` gives a field of default `default` at `position` (its default without a
/// link).
fn link_value(link: Option<&Link>, default: Value, position: u8) -> Value {
    link.map_or(default, |l| l.value(default, position))
}

fn leaf_or_variant(value: &dyn PartialReflect) -> Option<Value> {
    let mut found = None;
    walk(value, &mut String::new(), &mut |path, value, _| {
        if path.is_empty() {
            found = Some(value);
        }
    });
    found
}

/// The game's tuning resources, in the order the panel shows them, and the « Ressenti » knobs
/// over them (issue #155).
#[derive(Resource, Default)]
pub struct Tunings {
    pub list: Vec<Tuning>,
    /// Where the knobs stand (50 everywhere: the defaults). Not a Resource of its own: one more
    /// Resource in the headless world would be one more archetype before the grid, and every
    /// race of bots would change (see `STATUS.md`, « Comment ajouter une vérification de course »).
    pub feel: Feel,
}

/// First line of an export.
pub const EXPORT_HEADER: &str =
    "// Fluffy Racers : réglages du panneau d'admin (#36, #155), le reste est par défaut.";

impl Tunings {
    /// Adds a tuning resource (it must derive `Reflect` and `Default`).
    pub fn register<
        T: Resource + Component<Mutability = Mutable> + Reflect + Default + TypePath,
    >(
        &mut self,
    ) -> &mut Self {
        self.list.push(Tuning::of::<T>());
        self
    }

    pub fn get(&self, name: &str) -> Option<&Tuning> {
        self.list.iter().find(|t| t.name == name)
    }

    /// Every value modified by hand (off its baseline: what the knobs give it, its default when
    /// they are in the middle), tuning by tuning, in declaration order.
    pub fn changes(&self, world: &World) -> Vec<Change> {
        let mut out = Vec::new();
        for tuning in &self.list {
            let Some(resource) = tuning.read(world) else {
                continue;
            };
            for (i, field) in fields(resource.as_partial_reflect())
                .into_iter()
                .enumerate()
            {
                let baseline = tuning.baseline_at(&self.feel, i, &field.path);
                match baseline {
                    Some((baseline, _, _)) if baseline == field.value => {}
                    Some((_, default, driven)) => out.push(Change {
                        tuning: tuning.name,
                        path: field.path,
                        value: field.value,
                        default,
                        driven,
                    }),
                    None => out.push(Change {
                        tuning: tuning.name,
                        path: field.path,
                        value: field.value,
                        default: field.value,
                        driven: None,
                    }),
                }
            }
        }
        out
    }

    /// Nothing to keep: every knob in the middle and no value modified by hand.
    pub fn untouched(&self, world: &World) -> bool {
        self.feel.is_middle() && self.changes(world).is_empty()
    }

    /// The knobs off the middle and the values modified by hand as RON text, to paste in an issue.
    /// Empty map when nothing changed.
    pub fn export(&self, world: &World) -> String {
        export_text(&self.feel, &self.changes(world))
    }

    /// Writes `value` at `full_path` (`"KartTuning.max_speed"`).
    pub fn set(&self, world: &mut World, full_path: &str, value: Value) -> Result<(), SetError> {
        let (name, path) = full_path
            .split_once('.')
            .ok_or_else(|| SetError::UnknownField(full_path.to_string()))?;
        let tuning = self
            .get(name)
            .ok_or_else(|| SetError::UnknownField(full_path.to_string()))?;
        tuning.set(world, path, value)
    }

    /// Moves `knob` to `position` (0 to 100): every value it drives follows, except those
    /// modified by hand (off what the knob gave them at its previous position), which keep their
    /// value. Returns how many values moved.
    pub fn set_knob(&mut self, world: &mut World, knob: Knob, position: u8) -> usize {
        let position = position.min(KNOB_MAX);
        let before = self.feel.position(knob);
        if before == position {
            return 0;
        }
        let mut moved = 0;
        let mut edits: Vec<(String, Value)> = Vec::new();
        for tuning in &self.list {
            edits.clear();
            let Some(resource) = tuning.read(world) else {
                continue;
            };
            for (i, field) in fields(resource.as_partial_reflect())
                .into_iter()
                .enumerate()
            {
                let Some((default, Some(link))) = tuning.default_and_link(i, &field.path) else {
                    continue;
                };
                if link.knob != knob || field.value != link.value(default.value, before) {
                    continue;
                }
                let value = link.value(default.value, position);
                if value != field.value {
                    edits.push((field.path, value));
                }
            }
            for (path, value) in edits.drain(..) {
                if tuning.set(world, &path, value).is_ok() {
                    moved += 1;
                }
            }
        }
        self.feel.set(knob, position);
        moved
    }

    /// The field `path` of `tuning` back to its baseline: its knob drives it again.
    pub fn reset_field(&self, world: &mut World, tuning: &str, path: &str) -> Result<(), SetError> {
        let full = || format!("{tuning}.{path}");
        let tuning = self
            .get(tuning)
            .ok_or_else(|| SetError::UnknownField(full()))?;
        let baseline = tuning
            .baseline(&self.feel, path)
            .ok_or_else(|| SetError::UnknownField(full()))?;
        tuning.set(world, path, baseline)
    }

    /// The whole resource `name` back to its baselines: its defaults, and what the knobs give the
    /// values they drive.
    pub fn reset_tuning(&self, world: &mut World, name: &str) {
        let Some(tuning) = self.get(name) else {
            return;
        };
        tuning.reset(world);
        if self.feel.is_middle() {
            return;
        }
        let driven: Vec<(String, Value)> = tuning
            .defaults
            .iter()
            .enumerate()
            .filter_map(|(i, field)| {
                let (baseline, default, _) = tuning.baseline_at(&self.feel, i, &field.path)?;
                (baseline != default).then(|| (field.path.clone(), baseline))
            })
            .collect();
        for (path, value) in driven {
            let _ = tuning.set(world, &path, value);
        }
    }

    /// Every tuning back to its defaults, every knob back to the middle.
    pub fn reset_all(&mut self, world: &mut World) {
        for tuning in &self.list {
            tuning.reset(world);
        }
        self.feel = Feel::default();
    }

    /// Loads a text of [`Tunings::export`]: every tuning back to its defaults and every knob to
    /// the middle, then the knobs of the text (their values follow), then its values. Entries
    /// that do not fit (a renamed field or knob, a wrong value) are left out and listed in the
    /// report; a text that is not RON changes nothing. A text without knobs (before #155) loads
    /// as it did.
    pub fn import(&mut self, world: &mut World, text: &str) -> Result<ImportReport, String> {
        let entries = parse(text)?;
        self.reset_all(world);
        let mut report = ImportReport::default();
        for (full_path, raw) in &entries {
            let Some(key) = full_path.strip_prefix(FEEL_PREFIX) else {
                continue;
            };
            let Some(knob) = Knob::from_key(key) else {
                report
                    .ignored
                    .push(format!("{full_path} : réglage inconnu"));
                continue;
            };
            let position = match *raw {
                Raw::Int(i) if (0..=i128::from(KNOB_MAX)).contains(&i) => i as u8,
                Raw::Float(x) if (0.0..=f64::from(KNOB_MAX)).contains(&x) => x.round() as u8,
                _ => {
                    report
                        .ignored
                        .push(format!("{full_path} : valeur {raw:?} refusée"));
                    continue;
                }
            };
            self.set_knob(world, knob, position);
            report.applied += 1;
        }
        for (full_path, raw) in entries {
            if full_path.starts_with(FEEL_PREFIX) {
                continue;
            }
            let Some((name, path)) = full_path.split_once('.') else {
                report
                    .ignored
                    .push(format!("{full_path} : réglage inconnu"));
                continue;
            };
            let field = self
                .get(name)
                .and_then(|t| t.read(world))
                .and_then(|r| r.reflect_path(path).ok())
                .and_then(|value| {
                    let mut fields = fields(value);
                    // A leaf, not a whole struct.
                    (fields.len() == 1 && fields[0].path.is_empty()).then(|| fields.remove(0))
                });
            let Some(field) = field else {
                report
                    .ignored
                    .push(format!("{full_path} : réglage inconnu"));
                continue;
            };
            let Some(value) = raw.to_value(&field) else {
                report
                    .ignored
                    .push(format!("{full_path} : valeur {raw:?} refusée"));
                continue;
            };
            match self.set(world, &full_path, value) {
                Ok(()) => report.applied += 1,
                Err(error) => report.ignored.push(error.to_string()),
            }
        }
        Ok(report)
    }
}

/// The RON text of the knobs of `feel` off the middle and of `changes` (see the module
/// documentation).
pub fn export_text(feel: &Feel, changes: &[Change]) -> String {
    let mut text = String::new();
    text.push_str(EXPORT_HEADER);
    text.push('\n');
    if changes.is_empty() && feel.is_middle() {
        text.push_str("{}\n");
        return text;
    }
    text.push_str("{\n");
    for (knob, position) in feel.moved() {
        let _ = writeln!(
            text,
            "    \"{FEEL_PREFIX}{}\": {position}, // de 0 à {KNOB_MAX}, milieu : {MIDDLE}",
            knob.key()
        );
    }
    for change in changes {
        let _ = write!(
            text,
            "    \"{}.{}\": {}, // ",
            change.tuning, change.path, change.value
        );
        if let Some((knob, value)) = change.driven {
            let _ = write!(text, "curseur {} : {value}, ", knob.label());
        }
        let _ = writeln!(text, "défaut : {}", change.default);
    }
    text.push_str("}\n");
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bots::{BotDifficulty, BotTuning};
    use crate::kart::{KartTuning, MiniTurbo};

    fn world() -> (World, Tunings) {
        let mut world = World::new();
        world.init_resource::<KartTuning>();
        world.init_resource::<BotTuning>();
        let mut tunings = Tunings::default();
        tunings.register::<KartTuning>().register::<BotTuning>();
        (world, tunings)
    }

    #[test]
    fn every_field_is_found_by_reflection_with_its_path() {
        let kart = fields(&KartTuning::default());
        let find = |path: &str| kart.iter().find(|f| f.path == path).map(|f| f.value);
        assert_eq!(find("max_speed"), Some(Value::F32(28.5)));
        assert_eq!(find("airborne_grace_ticks"), Some(Value::U32(4)));
        assert_eq!(find("inertia.y"), Some(Value::F32(120.0)));
        assert_eq!(find("mini_turbo_thresholds[1]"), Some(Value::F32(570.0)));
        assert_eq!(find("drift_radius_tight"), Some(Value::F32(17.5)));
        // Every f32 and u32 of the struct, the 3 of `inertia` and the 3 of each array.
        assert!(kart.len() > 70, "{}", kart.len());

        let bots = fields(&BotTuning::default());
        // Issue #153: the level of the grid's bots is the grid's (`RaceGrid`), not a tuning.
        assert!(!bots.iter().any(|f| f.path == "grid_difficulty"));
        let tier = bots
            .iter()
            .find(|f| f.path == "skills[2].target_tier")
            .unwrap();
        assert_eq!(tier.value, Value::Variant("Orange"));
        assert_eq!(tier.variants, &["Blue", "Orange", "Purple"]);
        assert!(
            bots.iter().any(
                |f| f.path == "skills[0].purple_when_possible" && f.value == Value::Bool(false)
            )
        );
    }

    #[test]
    fn values_are_written_by_path_and_checked() {
        let mut tuning = BotTuning::default();
        set(&mut tuning, "skills[1].wobble", Value::F32(0.3)).unwrap();
        set(&mut tuning, "skills[1].target_tier", Value::Variant("Blue")).unwrap();
        set(
            &mut tuning,
            "skills[0].target_tier",
            Value::Variant("Purple"),
        )
        .unwrap();
        set(
            &mut tuning,
            "skills[2].purple_when_possible",
            Value::Bool(false),
        )
        .unwrap();
        assert_eq!(tuning.skills[1].wobble, 0.3);
        assert_eq!(
            tuning.skill(BotDifficulty::Normal).target_tier,
            MiniTurbo::Blue
        );
        assert_eq!(tuning.skills[0].target_tier, MiniTurbo::Purple);
        assert!(!tuning.skills[2].purple_when_possible);
        assert_eq!(
            set(&mut tuning, "skills[1].wobbel", Value::F32(1.0)),
            Err(SetError::UnknownField("skills[1].wobbel".into()))
        );
        assert_eq!(
            set(&mut tuning, "skills[1].wobble", Value::U32(1)),
            Err(SetError::WrongKind("skills[1].wobble".into()))
        );
        assert_eq!(
            set(
                &mut tuning,
                "skills[1].target_tier",
                Value::Variant("Nightmare")
            ),
            Err(SetError::UnknownVariant("skills[1].target_tier".into()))
        );
        // Nothing else moved.
        let mut expected = BotTuning::default();
        expected.skills[1].wobble = 0.3;
        expected.skills[1].target_tier = MiniTurbo::Blue;
        expected.skills[0].target_tier = MiniTurbo::Purple;
        expected.skills[2].purple_when_possible = false;
        assert_eq!(tuning, expected);
    }

    /// Acceptance criterion of #36: export then import gives the same values back, and the export
    /// only holds the modified values.
    #[test]
    fn the_export_holds_only_the_modified_values_and_imports_back() {
        let (mut world, mut tunings) = world();
        assert!(tunings.changes(&world).is_empty());
        let empty = tunings.export(&world);
        assert_eq!(parse(&empty).unwrap().len(), 0);

        let edits = [
            ("KartTuning.max_speed", Value::F32(31.25)),
            ("KartTuning.drift_curve_in_time", Value::F32(0.1)),
            ("KartTuning.inertia.y", Value::F32(99.5)),
            ("KartTuning.mini_turbo_thresholds[2]", Value::F32(900.0)),
            ("KartTuning.airborne_grace_ticks", Value::U32(7)),
            ("BotTuning.skills[1].target_tier", Value::Variant("Purple")),
            (
                "BotTuning.skills[0].purple_when_possible",
                Value::Bool(true),
            ),
            ("BotTuning.skills[2].target_tier", Value::Variant("Purple")),
        ];
        for (path, value) in edits {
            tunings.set(&mut world, path, value).unwrap();
        }
        let kart = world.resource::<KartTuning>().clone();
        let bots = world.resource::<BotTuning>().clone();
        let text = tunings.export(&world);

        let entries = parse(&text).unwrap();
        assert_eq!(entries.len(), edits.len(), "{text}");
        for (path, _) in edits {
            assert!(entries.contains_key(path), "{path} missing from\n{text}");
        }
        assert!(!text.contains("steer_rate"), "unmodified values: {text}");
        assert!(text.contains("\"KartTuning.max_speed\": 31.25, // défaut : 28.5"));
        assert!(
            text.contains(
                "\"BotTuning.skills[1].target_tier\": \"Purple\", // défaut : \"Orange\""
            )
        );
        assert!(text.contains("\"KartTuning.airborne_grace_ticks\": 7, // défaut : 4"));

        tunings.reset_all(&mut world);
        assert_eq!(*world.resource::<KartTuning>(), KartTuning::default());
        let report = tunings.import(&mut world, &text).unwrap();
        assert_eq!(report.applied, edits.len());
        assert!(report.ignored.is_empty(), "{:?}", report.ignored);
        assert_eq!(*world.resource::<KartTuning>(), kart);
        assert_eq!(*world.resource::<BotTuning>(), bots);
        assert_eq!(tunings.export(&world), text);
    }

    /// A text from an older version (renamed field, wrong value) or garbage never breaks the game:
    /// what fits is loaded, the rest is listed.
    #[test]
    fn an_import_skips_what_does_not_fit() {
        let (mut world, mut tunings) = world();
        tunings
            .set(&mut world, "KartTuning.coast_decel", Value::F32(1.0))
            .unwrap();
        let text = r#"{
            "KartTuning.max_speed": 30, // an integer for a float is fine
            "KartTuning.old_name": 3.0,
            "Nope.max_speed": 3.0,
            "KartTuning.airborne_grace_ticks": -2,
            "KartTuning.hop_min_ticks": 2.5,
            "KartTuning.inertia": 3.0,
            "BotTuning.grid_difficulty": "Nightmare", // a field gone since #153
            "BotTuning.skills[1].drift_chance": true,
            "BotTuning.skills[1].wobble": 0.25,
        }"#;
        let report = tunings.import(&mut world, text).unwrap();
        assert_eq!(report.applied, 2, "{:?}", report.ignored);
        assert_eq!(report.ignored.len(), 7, "{:?}", report.ignored);
        let kart = world.resource::<KartTuning>();
        assert_eq!(kart.max_speed, 30.0);
        // Everything else back to the defaults (the import starts from them).
        assert_eq!(kart.coast_decel, KartTuning::default().coast_decel);
        assert_eq!(world.resource::<BotTuning>().skills[1].wobble, 0.25);

        // Not RON at all: an error, and nothing changes.
        tunings
            .set(&mut world, "KartTuning.coast_decel", Value::F32(1.0))
            .unwrap();
        assert!(tunings.import(&mut world, "max_speed = 30").is_err());
        assert_eq!(world.resource::<KartTuning>().coast_decel, 1.0);
        // An empty text resets everything.
        assert_eq!(tunings.import(&mut world, "  ").unwrap().applied, 0);
        assert_eq!(*world.resource::<KartTuning>(), KartTuning::default());
    }

    #[test]
    fn writing_the_same_value_does_not_mark_the_resource_changed() {
        let (mut world, tunings) = world();
        world.clear_trackers();
        tunings
            .set(&mut world, "KartTuning.max_speed", Value::F32(28.5))
            .unwrap();
        assert!(!world.is_resource_changed::<KartTuning>());
        tunings
            .set(&mut world, "KartTuning.max_speed", Value::F32(29.0))
            .unwrap();
        assert!(world.is_resource_changed::<KartTuning>());
    }

    #[test]
    fn numbers_keep_their_kind() {
        assert_eq!(Value::U8(3).with_number(300.4), Some(Value::U8(255)));
        assert_eq!(Value::U32(3).with_number(-2.0), Some(Value::U32(0)));
        assert_eq!(Value::U32(3).with_number(6.6), Some(Value::U32(7)));
        assert_eq!(Value::F32(0.0).with_number(0.25), Some(Value::F32(0.25)));
        assert_eq!(Value::Bool(true).with_number(1.0), None);
        assert!(Value::U8(1).is_integer() && !Value::F32(1.0).is_integer());
        assert_eq!(Value::F32(30.0).to_string(), "30.0");
        assert_eq!(Value::F32(0.1).to_string(), "0.1");
        assert_eq!(Value::Variant("Hard").to_string(), "\"Hard\"");
    }
}
