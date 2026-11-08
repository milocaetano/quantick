//! Arena-owned drawing records. The public store is regenerated output only.
use super::{Arena, ScriptState, Value};
use quantick_indicators::{
    BoxObj, LabelObj, LabelStyle, LineObj, MAX_OBJECTS_PER_KIND, ObjectId, ObjectStore, Rgba8,
};
use std::{
    ops::{Deref, DerefMut},
    rc::Rc,
};

const OBJECTS: usize = 21;
pub(super) const NEXT_ID: usize = 22;
const REVISION: usize = 23;

fn index(word: u64) -> usize {
    usize::try_from(word).expect("object offset")
}
fn num(value: &Value) -> f64 {
    value.as_num().expect("object numeric field")
}
fn color(value: &Value) -> Rgba8 {
    let Value::Color(c) = value else {
        unreachable!("object color")
    };
    Rgba8::from_u32(*c)
}
fn integer(value: i64) -> Value {
    Value::Num(f64::from_bits(u64::from_ne_bytes(value.to_ne_bytes())))
}
fn integer_value(value: &Value) -> i64 {
    i64::from_ne_bytes(num(value).to_bits().to_ne_bytes())
}

pub(super) trait Record: Sized {
    const KIND: u64;
    fn encode(&self) -> Value;
    fn decode(value: Value) -> Self;
}

impl Record for LineObj {
    const KIND: u64 = 0;
    fn encode(&self) -> Value {
        Value::Tuple(Rc::new(vec![
            integer(self.x1),
            Value::Num(self.y1),
            integer(self.x2),
            Value::Num(self.y2),
            Value::Color(self.color.to_u32()),
            Value::Num(f64::from(self.width)),
        ]))
    }
    fn decode(value: Value) -> Self {
        let Value::Tuple(v) = value else {
            unreachable!("line record")
        };
        #[allow(clippy::cast_possible_truncation)]
        Self {
            x1: integer_value(&v[0]),
            y1: num(&v[1]),
            x2: integer_value(&v[2]),
            y2: num(&v[3]),
            color: color(&v[4]),
            width: num(&v[5]) as f32,
        }
    }
}
impl Record for BoxObj {
    const KIND: u64 = 1;
    fn encode(&self) -> Value {
        Value::Tuple(Rc::new(vec![
            integer(self.left),
            Value::Num(self.top),
            integer(self.right),
            Value::Num(self.bottom),
            Value::Color(self.border_color.to_u32()),
            Value::Color(self.bg_color.to_u32()),
            Value::Num(f64::from(self.border_width)),
        ]))
    }
    fn decode(value: Value) -> Self {
        let Value::Tuple(v) = value else {
            unreachable!("box record")
        };
        #[allow(clippy::cast_possible_truncation)]
        Self {
            left: integer_value(&v[0]),
            top: num(&v[1]),
            right: integer_value(&v[2]),
            bottom: num(&v[3]),
            border_color: color(&v[4]),
            bg_color: color(&v[5]),
            border_width: num(&v[6]) as f32,
        }
    }
}
impl Record for LabelObj {
    const KIND: u64 = 2;
    fn encode(&self) -> Value {
        Value::Tuple(Rc::new(vec![
            integer(self.x),
            Value::Num(self.y),
            Value::Str(Rc::new(self.text.clone())),
            Value::Color(self.color.to_u32()),
            Value::Color(self.text_color.to_u32()),
            Value::Num(match self.style {
                LabelStyle::Up => 0.0,
                LabelStyle::Down => 1.0,
                LabelStyle::None => 2.0,
            }),
        ]))
    }
    fn decode(value: Value) -> Self {
        let Value::Tuple(v) = value else {
            unreachable!("label record")
        };
        let Value::Str(text) = &v[2] else {
            unreachable!("label text")
        };
        Self {
            x: integer_value(&v[0]),
            y: num(&v[1]),
            text: text.as_ref().clone(),
            color: color(&v[3]),
            text_color: color(&v[4]),
            style: if num(&v[5]) == 0.0 {
                LabelStyle::Up
            } else if num(&v[5]) == 1.0 {
                LabelStyle::Down
            } else {
                LabelStyle::None
            },
        }
    }
}

pub(super) struct Edit<'a, T: Record> {
    arena: &'a mut Arena,
    record: usize,
    value: T,
}
impl<T: Record> Deref for Edit<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.value
    }
}
impl<T: Record> DerefMut for Edit<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.value
    }
}
impl<T: Record> Drop for Edit<'_, T> {
    fn drop(&mut self) {
        self.arena.set(self.record + 2, 0, self.value.encode());
    }
}

impl ScriptState {
    pub(super) fn new_object<T: Record>(&mut self, value: T) -> ObjectId {
        let id = u32::try_from(self.arena.words[NEXT_ID]).expect("object id");
        self.arena.words[NEXT_ID] += 1;
        self.arena.words[REVISION] += 1;
        let (record, _) = self.arena.record(OBJECTS, T::KIND << 32 | u64::from(id), 5);
        self.arena.resize(record + 2, 1);
        self.arena.set(record + 2, 0, value.encode());
        let mut count = 0;
        let mut link = OBJECTS;
        while self.arena.words[link] != 0 {
            let at = index(self.arena.words[link]);
            if self.arena.words[at + 1] >> 32 == T::KIND {
                count += 1;
                if count > MAX_OBJECTS_PER_KIND {
                    self.unlink_object(link, at);
                    break;
                }
            }
            link = at;
        }
        ObjectId(id)
    }
    fn unlink_object(&mut self, link: usize, record: usize) {
        self.arena.words[link] = self.arena.words[record];
        self.arena.resize(record + 2, 0);
        self.arena.release(index(self.arena.words[record + 2]));
        self.arena.release(record);
    }
    pub(super) fn delete_object<T: Record>(&mut self, id: ObjectId) {
        let key = T::KIND << 32 | u64::from(id.0);
        let mut link = OBJECTS;
        while self.arena.words[link] != 0 {
            let at = index(self.arena.words[link]);
            if self.arena.words[at + 1] == key {
                self.unlink_object(link, at);
                self.arena.words[REVISION] += 1;
                break;
            }
            link = at;
        }
    }
    pub(super) fn edit_object<T: Record>(&mut self, id: ObjectId) -> Option<Edit<'_, T>> {
        let key = T::KIND << 32 | u64::from(id.0);
        let mut record = index(self.arena.words[OBJECTS]);
        while record != 0 {
            if self.arena.words[record + 1] == key {
                let value = T::decode(self.arena.get(record + 2, 0));
                self.arena.words[REVISION] += 1;
                return Some(Edit {
                    arena: &mut self.arena,
                    record,
                    value,
                });
            }
            record = index(self.arena.words[record]);
        }
        None
    }
    pub(crate) fn project_objects(&mut self) {
        let mut lines = Vec::new();
        let mut boxes = Vec::new();
        let mut labels = Vec::new();
        let mut record = index(self.arena.words[OBJECTS]);
        while record != 0 {
            let key = self.arena.words[record + 1];
            let id = u32::try_from(key & u64::from(u32::MAX)).expect("object id");
            let value = self.arena.get(record + 2, 0);
            match key >> 32 {
                0 => lines.push((id, LineObj::decode(value))),
                1 => boxes.push((id, BoxObj::decode(value))),
                2 => labels.push((id, LabelObj::decode(value))),
                _ => unreachable!("object kind"),
            }
            record = index(self.arena.words[record]);
        }
        lines.reverse();
        boxes.reverse();
        labels.reverse();
        self.objects = ObjectStore::from_records(
            lines,
            boxes,
            labels,
            u32::try_from(self.arena.words[NEXT_ID]).expect("object counter"),
            self.arena.words[REVISION],
        );
    }
}
